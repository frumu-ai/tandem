match action {
            Action::Quit => self.should_quit = true,
            Action::CtrlCPressed => {
                let now = Instant::now();
                if self
                    .quit_armed_at
                    .map(|t| now.duration_since(t).as_millis() <= 1500)
                    .unwrap_or(false)
                {
                    self.should_quit = true;
                    self.quit_armed_at = None;
                } else {
                    self.quit_armed_at = Some(now);
                    let mut cancelled = false;
                    if let AppState::Chat {
                        active_agent_index,
                        agents,
                        ..
                    } = &self.state
                    {
                        if *active_agent_index < agents.len()
                            && agents[*active_agent_index].active_run_id.is_some()
                        {
                            self.cancel_agent_if_running(*active_agent_index).await;
                            cancelled = true;
                        }
                    }
                    if let AppState::Chat { messages, .. } = &mut self.state {
                        let notice = if cancelled {
                            "Cancelled active run. Press Ctrl+C again within 1.5s to quit."
                        } else {
                            "Press Ctrl+C again within 1.5s to quit."
                        };
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(notice.to_string())],
                        });
                    }
                }
            }
            Action::SkipAnimation => {
                if let AppState::StartupAnimation { .. } = self.state {
                    self.state = AppState::PinPrompt {
                        input: String::new(),
                        error: None,
                        // If a vault key exists, unlock flow should always be used.
                        // An empty/missing keystore can be recreated after successful unlock.
                        mode: if self.vault_key.is_some() {
                            PinPromptMode::UnlockExisting
                        } else {
                            PinPromptMode::CreateNew
                        },
                    };
                }
            }
            Action::ToggleTaskPin(task_id) => {
                if let AppState::Chat { tasks, .. } = &mut self.state {
                    if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                        task.pinned = !task.pinned;
                    }
                }
            }

            Action::Tick => self.tick().await,

            Action::EnterPin(c) => {
                if let AppState::PinPrompt { input, .. } = &mut self.state {
                    if c == '\x08' {
                        input.pop();
                    } else if c.is_ascii_digit() && input.len() < MAX_PIN_LENGTH {
                        input.push(c);
                    }
                }
            }
            Action::SubmitPin => {
                let (input, mode) = match &self.state {
                    AppState::PinPrompt { input, mode, .. } => (input.clone(), mode.clone()),
                    _ => (String::new(), PinPromptMode::UnlockExisting),
                };

                match mode {
                    PinPromptMode::UnlockExisting => {
                        if let Err(e) = crate::crypto::vault::validate_pin_format(&input) {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some(e.to_string()),
                                mode: PinPromptMode::UnlockExisting,
                            };
                            return Ok(());
                        }
                        match &self.vault_key {
                            Some(vk) => match vk.decrypt(&input) {
                                Ok(master_key) => {
                                    if let Some(config_dir) = &self.config_dir {
                                        let keystore_path = config_dir.join("tandem.keystore");
                                        match SecureKeyStore::load(&keystore_path, master_key) {
                                            Ok(store) => {
                                                // Ensure keystore file exists on disk for first-time users.
                                                if let Err(e) = store.save(&keystore_path) {
                                                    self.state = AppState::PinPrompt {
                                                        input: String::new(),
                                                        error: Some(format!(
                                                            "Failed to save keystore: {}",
                                                            e
                                                        )),
                                                        mode: PinPromptMode::UnlockExisting,
                                                    };
                                                    return Ok(());
                                                }
                                                self.keystore = Some(store);
                                                self.state = AppState::Connecting;
                                                return Ok(());
                                            }
                                            Err(_) => {
                                                self.state = AppState::PinPrompt {
                                                    input: String::new(),
                                                    error: Some(
                                                        "Failed to load keystore".to_string(),
                                                    ),
                                                    mode: PinPromptMode::UnlockExisting,
                                                };
                                            }
                                        }
                                    } else {
                                        self.state = AppState::PinPrompt {
                                            input: String::new(),
                                            error: Some("Config dir not found".to_string()),
                                            mode: PinPromptMode::UnlockExisting,
                                        };
                                    }
                                }
                                Err(_) => {
                                    self.state = AppState::PinPrompt {
                                        input: String::new(),
                                        error: Some("Invalid PIN".to_string()),
                                        mode: PinPromptMode::UnlockExisting,
                                    };
                                }
                            },
                            None => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: Some(
                                        "No vault key found. Create a new PIN.".to_string(),
                                    ),
                                    mode: PinPromptMode::CreateNew,
                                };
                            }
                        }
                    }
                    PinPromptMode::CreateNew => {
                        match crate::crypto::vault::validate_pin_format(&input) {
                            Ok(_) => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: None,
                                    mode: PinPromptMode::ConfirmNew { first_pin: input },
                                };
                            }
                            Err(e) => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: Some(e.to_string()),
                                    mode: PinPromptMode::CreateNew,
                                };
                            }
                        }
                    }
                    PinPromptMode::ConfirmNew { first_pin } => {
                        if let Err(e) = crate::crypto::vault::validate_pin_format(&input) {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some(e.to_string()),
                                mode: PinPromptMode::CreateNew,
                            };
                            return Ok(());
                        }
                        if input != first_pin {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some("PINs do not match. Enter a new PIN.".to_string()),
                                mode: PinPromptMode::CreateNew,
                            };
                            return Ok(());
                        }

                        if let Some(config_dir) = &self.config_dir {
                            let vault_path = config_dir.join("vault.key");
                            let keystore_path = config_dir.join("tandem.keystore");
                            match EncryptedVaultKey::create(&input) {
                                Ok((vault_key, master_key)) => {
                                    if let Err(e) = vault_key.save(&vault_path) {
                                        self.state = AppState::PinPrompt {
                                            input: String::new(),
                                            error: Some(format!("Failed to save vault: {}", e)),
                                            mode: PinPromptMode::CreateNew,
                                        };
                                        return Ok(());
                                    }

                                    match SecureKeyStore::load(&keystore_path, master_key) {
                                        Ok(store) => {
                                            if let Err(e) = store.save(&keystore_path) {
                                                self.state = AppState::PinPrompt {
                                                    input: String::new(),
                                                    error: Some(format!(
                                                        "Failed to save keystore: {}",
                                                        e
                                                    )),
                                                    mode: PinPromptMode::CreateNew,
                                                };
                                                return Ok(());
                                            }
                                            self.vault_key = Some(vault_key);
                                            self.keystore = Some(store);
                                            self.state = AppState::Connecting;
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            self.state = AppState::PinPrompt {
                                                input: String::new(),
                                                error: Some(format!(
                                                    "Failed to initialize keystore: {}",
                                                    e
                                                )),
                                                mode: PinPromptMode::CreateNew,
                                            };
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.state = AppState::PinPrompt {
                                        input: String::new(),
                                        error: Some(format!("Failed to create vault: {}", e)),
                                        mode: PinPromptMode::CreateNew,
                                    };
                                }
                            }
                        } else {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some("Config dir not found".to_string()),
                                mode: PinPromptMode::CreateNew,
                            };
                        }
                    }
                }
            }

            Action::SessionsLoaded(sessions) => {
                self.sessions = sessions;
                if self.selected_session_index >= self.sessions.len() && !self.sessions.is_empty() {
                    self.selected_session_index = self.sessions.len() - 1;
                }
            }
            Action::NextSession => {
                if !self.sessions.is_empty() {
                    self.selected_session_index =
                        (self.selected_session_index + 1) % self.sessions.len();
                }
            }
            Action::PreviousSession => {
                if !self.sessions.is_empty() {
                    if self.selected_session_index > 0 {
                        self.selected_session_index -= 1;
                    } else {
                        self.selected_session_index = self.sessions.len() - 1;
                    }
                }
            }
            Action::NewSession => {
                // If configuration is missing, force wizard
                if (self.current_provider.is_none() || self.current_model.is_none())
                    && self.provider_catalog.is_some()
                {
                    let mut step = SetupStep::SelectProvider;
                    let mut selected_provider_index = 0;

                    if let Some(ref current_p) = self.current_provider {
                        if let Some(ref catalog) = self.provider_catalog {
                            if let Some(idx) = catalog.all.iter().position(|p| &p.id == current_p) {
                                selected_provider_index = idx;
                                if self.current_model.is_none() {
                                    step = SetupStep::SelectModel;
                                }
                            }
                        }
                    }

                    self.state = AppState::SetupWizard {
                        step,
                        provider_catalog: self.provider_catalog.clone(),
                        selected_provider_index,
                        selected_model_index: 0,
                        api_key_input: String::new(),
                        model_input: String::new(),
                    };
                    return Ok(());
                }

                if let Some(client) = &self.client {
                    let client = client.clone();
                    // We can't await easily here if update locks self?
                    // Actually update is async, so we can await.
                    // But we hold &mut self.
                    // client clone allows us to call it.
                    // But we can't assign to self.sessions *after* await while holding client?
                    // No, `client` is a local variable. `self` is currently borrowed.
                    // We can't call methods on self.

                    if let Ok(_) = client.create_session(Some("New session".to_string())).await {
                        // Refresh sessions
                        if let Ok(sessions) = client.list_sessions().await {
                            self.sessions = sessions;
                            // Select the new one (usually first or last depending on sort)
                            // server sorts by updated desc, so new one is first.
                            self.selected_session_index = 0;
                            if let Some(ref session) = self.sessions.first() {
                                let first_agent =
                                    Self::make_agent_pane("A1".to_string(), session.id.clone());
                                self.state = AppState::Chat {
                                    session_id: session.id.clone(),
                                    command_input: ComposerInputState::new(),
                                    messages: Vec::new(),
                                    scroll_from_bottom: 0,
                                    tasks: Vec::new(),
                                    active_task_id: None,
                                    agents: vec![first_agent],
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
                                };
                            }
                        }
                    }
                }
            }

            Action::SelectSession => {
                if !self.sessions.is_empty() {
                    let session = &self.sessions[self.selected_session_index];
                    let loaded_messages = self.load_chat_history(&session.id).await;
                    let (recalled_tasks, recalled_active_task_id) =
                        plan_helpers::rebuild_tasks_from_messages(&loaded_messages);
                    let mut first_agent =
                        Self::make_agent_pane("A1".to_string(), session.id.clone());
                    first_agent.messages = loaded_messages.clone();
                    first_agent.tasks = recalled_tasks.clone();
                    first_agent.active_task_id = recalled_active_task_id.clone();
                    self.state = AppState::Chat {
                        session_id: session.id.clone(),
                        command_input: ComposerInputState::new(),
                        messages: loaded_messages,
                        scroll_from_bottom: 0,
                        tasks: recalled_tasks,
                        active_task_id: recalled_active_task_id,
                        agents: vec![first_agent],
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
                    };
                }
            }
            Action::DeleteSelectedSession => {
                if self.sessions.is_empty() {
                    self.connection_status = "No session selected to delete.".to_string();
                    return Ok(());
                }
                let selected_index = self.selected_session_index.min(self.sessions.len() - 1);
                let selected = self.sessions[selected_index].clone();
                if let Some(client) = &self.client {
                    match client.delete_session(&selected.id).await {
                        Ok(_) => {
                            self.sessions.remove(selected_index);
                            if self.sessions.is_empty() {
                                self.selected_session_index = 0;
                            } else if self.selected_session_index >= self.sessions.len() {
                                self.selected_session_index = self.sessions.len() - 1;
                            }
                            self.connection_status =
                                format!("Deleted session: {} ({})", selected.title, selected.id);
                        }
                        Err(err) => {
                            self.connection_status =
                                format!("Failed to delete session {}: {}", selected.id, err);
                        }
                    }
                } else {
                    self.connection_status = "Not connected to engine".to_string();
                }
            }

            Action::CommandInput(c) => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.insert_char(c);
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }

            Action::BackspaceCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    if let Some((start, end)) = Self::paste_token_range_for_backspace(command_input)
                    {
                        command_input.remove_range(start, end);
                    } else {
                        command_input.backspace();
                    }
                    let input = command_input.text().to_string();
                    if input == "/" {
                        self.autocomplete_items = COMMAND_HELP
                            .iter()
                            .map(|(name, desc)| (name.to_string(), desc.to_string()))
                            .collect();
                        self.autocomplete_index = 0;
                        self.autocomplete_mode = AutocompleteMode::Command;
                        self.show_autocomplete = true;
                    } else {
                        self.update_autocomplete_for_input(&input);
                    }
                }
                self.sync_active_agent_from_chat();
            }
            Action::DeleteForwardCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    if let Some((start, end)) = Self::paste_token_range_for_delete(command_input) {
                        command_input.remove_range(start, end);
                    } else {
                        command_input.delete_forward();
                    }
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }
            Action::InsertNewline => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.insert_char('\n');
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorLeft => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_left();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorRight => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_right();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorHome => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_home();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorEnd => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_end();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorUp => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_line_up();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorDown => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_line_down();
                }
                self.sync_active_agent_from_chat();
            }
            Action::PasteFromClipboard => {
                match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    Ok(text) => {
                        let normalized = Self::normalize_paste_payload(&text);
                        if !normalized.is_empty() {
                            match &mut self.state {
                                AppState::Chat {
                                    command_input,
                                    agents,
                                    active_agent_index,
                                    ..
                                } => {
                                    let inserted = Self::insert_chat_paste(
                                        agents.get_mut(*active_agent_index),
                                        &normalized,
                                    );
                                    command_input.insert_str(&inserted);
                                    let input = command_input.text().to_string();
                                    if let Some(agent) = agents.get_mut(*active_agent_index) {
                                        agent.draft = command_input.clone();
                                        Self::prune_agent_paste_registry(agent);
                                    }
                                    self.update_autocomplete_for_input(&input);
                                }
                                AppState::SetupWizard {
                                    step,
                                    api_key_input,
                                    model_input,
                                    ..
                                } => match step {
                                    SetupStep::EnterApiKey => {
                                        api_key_input
                                            .push_str(normalized.trim_end_matches(['\n', '\r']));
                                    }
                                    SetupStep::SelectModel => {
                                        model_input
                                            .push_str(normalized.trim_end_matches(['\n', '\r']));
                                    }
                                    _ => {}
                                },
                                _ => {}
                            }
                            self.sync_active_agent_from_chat();
                        }
                    }
                    Err(err) => {
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(format!(
                                    "Clipboard paste failed: {}",
                                    err
                                ))],
                            });
                        }
                    }
                }
            }
            Action::PasteInput(text) => {
                let normalized = Self::normalize_paste_payload(&text);
                match &mut self.state {
                    AppState::Chat {
                        command_input,
                        agents,
                        active_agent_index,
                        ..
                    } => {
                        let inserted = Self::insert_chat_paste(
                            agents.get_mut(*active_agent_index),
                            &normalized,
                        );
                        command_input.insert_str(&inserted);
                        let input = command_input.text().to_string();
                        if let Some(agent) = agents.get_mut(*active_agent_index) {
                            agent.draft = command_input.clone();
                            Self::prune_agent_paste_registry(agent);
                        }
                        self.update_autocomplete_for_input(&input);
                    }
                    AppState::SetupWizard {
                        step,
                        api_key_input,
                        model_input,
                        ..
                    } => match step {
                        SetupStep::EnterApiKey => {
                            api_key_input.push_str(normalized.trim_end_matches(['\n', '\r']));
                        }
                        SetupStep::SelectModel => {
                            model_input.push_str(normalized.trim_end_matches(['\n', '\r']));
                        }
                        _ => {}
                    },
                    _ => {}
                }
                self.sync_active_agent_from_chat();
            }

            Action::Autocomplete => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    if !command_input.text().starts_with('/') {
                        command_input.clear();
                        command_input.insert_char('/');
                    }
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
            }

            Action::AutocompleteNext => {
                if !self.autocomplete_items.is_empty() {
                    self.autocomplete_index =
                        (self.autocomplete_index + 1) % self.autocomplete_items.len();
                }
            }

            Action::AutocompletePrev => {
                if !self.autocomplete_items.is_empty() {
                    if self.autocomplete_index > 0 {
                        self.autocomplete_index -= 1;
                    } else {
                        self.autocomplete_index = self.autocomplete_items.len() - 1;
                    }
                }
            }

            Action::AutocompleteAccept => {
                if self.show_autocomplete && !self.autocomplete_items.is_empty() {
                    let (cmd, _) = self.autocomplete_items[self.autocomplete_index].clone();
                    if let AppState::Chat { command_input, .. } = &mut self.state {
                        command_input.clear();
                        match self.autocomplete_mode {
                            AutocompleteMode::Command => {
                                command_input.set_text(format!("/{} ", cmd));
                            }
                            AutocompleteMode::Provider => {
                                command_input.set_text(format!("/provider {}", cmd));
                            }
                            AutocompleteMode::Model => {
                                command_input.set_text(format!("/model {}", cmd));
                            }
                        }
                        command_input.move_end();
                    }
                    self.show_autocomplete = false;
                    self.autocomplete_items.clear();
                }
                self.sync_active_agent_from_chat();
            }

            Action::AutocompleteDismiss => {
                self.show_autocomplete = false;
                self.autocomplete_items.clear();
                self.autocomplete_mode = AutocompleteMode::Command;
            }

            Action::BackToMenu => {
                self.show_autocomplete = false;
                self.autocomplete_items.clear();
                self.autocomplete_mode = AutocompleteMode::Command;
                self.state = AppState::MainMenu;
            }

            Action::SwitchAgentNext => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if !agents.is_empty() {
                        *active_agent_index = (*active_agent_index + 1) % agents.len();
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::SwitchAgentPrev => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if !agents.is_empty() {
                        if *active_agent_index == 0 {
                            *active_agent_index = agents.len().saturating_sub(1);
                        } else {
                            *active_agent_index -= 1;
                        }
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::SelectAgentByNumber(n) => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if n > 0 && n <= agents.len() {
                        *active_agent_index = n - 1;
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::ToggleUiMode => {
                if let AppState::Chat { ui_mode, .. } = &mut self.state {
                    *ui_mode = if *ui_mode == UiMode::Focus {
                        UiMode::Grid
                    } else {
                        UiMode::Focus
                    };
                }
            }
            Action::CycleMode => {
                self.current_mode = self.current_mode.next();
            }
            Action::GridPageNext => {
                if let AppState::Chat {
                    grid_page, agents, ..
                } = &mut self.state
                {
                    let max_page = agents.len().saturating_sub(1) / 4;
                    *grid_page = (*grid_page + 1).min(max_page);
                }
            }
            Action::GridPagePrev => {
                if let AppState::Chat { grid_page, .. } = &mut self.state {
                    *grid_page = grid_page.saturating_sub(1);
                }
            }
            Action::ShowHelpModal => {
                if let AppState::Chat { modal, .. } = &mut self.state {
                    *modal = Some(ModalState::Help);
                }
            }
            Action::OpenDocs => {
                // Open docs in default browser
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "start", "https://docs.tandem.ac/"])
                    .spawn();
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open")
                    .arg("https://docs.tandem.ac/")
                    .spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open")
                    .arg("https://docs.tandem.ac/")
                    .spawn();
            }
            Action::CopyLastAssistant => {
                let copied = if let AppState::Chat { messages, .. } = &self.state {
                    self.copy_latest_assistant_to_clipboard(messages)
                } else {
                    Err("Clipboard copy works in chat screens only.".to_string())
                };
                if let AppState::Chat { messages, .. } = &mut self.state {
                    let note = match copied {
                        Ok(len) => format!("Copied {} characters to clipboard.", len),
                        Err(err) => format!("Clipboard copy failed: {}", err),
                    };
                    messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: vec![ContentBlock::Text(note)],
                    });
                }
            }
            Action::CloseModal => {
                if let AppState::Chat { modal, .. } = &mut self.state {
                    if matches!(*modal, Some(ModalState::Pager)) {
                        self.pager_overlay = None;
                    }
                    *modal = None;
                }
                self.open_queued_plan_agent_prompt();
            }
            Action::OpenRequestCenter => {
                self.open_request_center_if_needed();
            }
            Action::OpenFileSearch => {
                self.open_file_search_modal(None);
            }
            Action::OpenDiffOverlay => {
                let status = self.open_diff_overlay().await;
                if let AppState::Chat { messages, .. } = &mut self.state {
                    messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: vec![ContentBlock::Text(status)],
                    });
                }
                self.sync_active_agent_from_chat();
            }
            Action::OpenExternalEditor => {
                let status = self.open_external_editor_for_active_input().await;
                if let AppState::Chat { messages, .. } = &mut self.state {
                    messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: vec![ContentBlock::Text(status)],
                    });
                }
                self.sync_active_agent_from_chat();
            }
            Action::ToggleRequestPanelExpand => {
                if let AppState::Chat {
                    request_panel_expanded,
                    ..
                } = &mut self.state
                {
                    *request_panel_expanded = !*request_panel_expanded;
                }
            }
            Action::RequestSelectNext => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if !pending_requests.is_empty() {
                        *request_cursor = (*request_cursor + 1) % pending_requests.len();
                        *permission_choice = 0;
                    }
                }
            }
            Action::RequestSelectPrev => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if !pending_requests.is_empty() {
                        *request_cursor = if *request_cursor == 0 {
                            pending_requests.len().saturating_sub(1)
                        } else {
                            request_cursor.saturating_sub(1)
                        };
                        *permission_choice = 0;
                    }
                }
            }
            Action::RequestOptionNext => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = (*permission_choice + 1) % 3;
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if !q.options.is_empty() {
                                        q.option_cursor = (q.option_cursor + 1) % q.options.len();
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestOptionPrev => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = if *permission_choice == 0 {
                                    2
                                } else {
                                    permission_choice.saturating_sub(1)
                                };
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if !q.options.is_empty() {
                                        q.option_cursor = if q.option_cursor == 0 {
                                            q.options.len().saturating_sub(1)
                                        } else {
                                            q.option_cursor.saturating_sub(1)
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::FileSearchInput(c) => {
                self.file_search.query.push(c);
                self.refresh_file_search_matches();
            }
            Action::FileSearchBackspace => {
                self.file_search.query.pop();
                self.refresh_file_search_matches();
            }
            Action::FileSearchSelectNext => {
                if !self.file_search.matches.is_empty() {
                    self.file_search.cursor =
                        (self.file_search.cursor + 1) % self.file_search.matches.len();
                }
            }
            Action::FileSearchSelectPrev => {
                if !self.file_search.matches.is_empty() {
                    self.file_search.cursor = if self.file_search.cursor == 0 {
                        self.file_search.matches.len().saturating_sub(1)
                    } else {
                        self.file_search.cursor.saturating_sub(1)
                    };
                }
            }
            Action::FileSearchConfirm => {
                if let Some(selected) = self.file_search.matches.get(self.file_search.cursor) {
                    if let AppState::Chat { command_input, .. } = &mut self.state {
                        if !command_input.text().is_empty() {
                            command_input.insert_char(' ');
                        }
                        command_input.insert_str(&format!("@{}", selected));
                    }
                    if let AppState::Chat { modal, .. } = &mut self.state {
                        *modal = None;
                    }
                    self.sync_active_agent_from_chat();
                }
            }
            Action::RequestToggleCurrent => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = (*permission_choice + 1) % 3;
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if q.option_cursor < q.options.len() {
                                        if q.multiple {
                                            if let Some(existing) = q
                                                .selected_options
                                                .iter()
                                                .position(|v| *v == q.option_cursor)
                                            {
                                                q.selected_options.remove(existing);
                                            } else {
                                                q.selected_options.push(q.option_cursor);
                                            }
                                        } else {
                                            q.selected_options = vec![q.option_cursor];
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestDigit(digit) => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                if (1..=3).contains(&digit) {
                                    *permission_choice = digit as usize - 1;
                                }
                            }
                            PendingRequestKind::Question(question) => {
                                let idx = digit.saturating_sub(1) as usize;
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if idx < q.options.len() {
                                        q.option_cursor = idx;
                                        if q.multiple {
                                            if let Some(existing) =
                                                q.selected_options.iter().position(|v| *v == idx)
                                            {
                                                q.selected_options.remove(existing);
                                            } else {
                                                q.selected_options.push(idx);
                                            }
                                        } else {
                                            q.selected_options = vec![idx];
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestInput(c) => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        if let PendingRequestKind::Question(question) = &mut request.kind {
                            if let Some(q) = question.questions.get_mut(question.question_index) {
                                if q.custom || !q.options.is_empty() {
                                    q.custom_input.push(c);
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestBackspace => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        if let PendingRequestKind::Question(question) = &mut request.kind {
                            if let Some(q) = question.questions.get_mut(question.question_index) {
                                q.custom_input.pop();
                            }
                        }
                    }
                }
            }
            Action::PlanWizardNextField => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    plan_wizard.cursor_step = (plan_wizard.cursor_step + 1) % 5;
                }
            }
            Action::PlanWizardPrevField => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    plan_wizard.cursor_step = if plan_wizard.cursor_step == 0 {
                        4
                    } else {
                        plan_wizard.cursor_step.saturating_sub(1)
                    };
                }
            }
            Action::PlanWizardInput(c) => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    let target = match plan_wizard.cursor_step {
                        0 => &mut plan_wizard.plan_name,
                        1 => &mut plan_wizard.scope,
                        2 => &mut plan_wizard.constraints,
                        3 => &mut plan_wizard.priorities,
                        _ => &mut plan_wizard.notes,
                    };
                    target.push(c);
                }
            }
            Action::PlanWizardBackspace => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    let target = match plan_wizard.cursor_step {
                        0 => &mut plan_wizard.plan_name,
                        1 => &mut plan_wizard.scope,
                        2 => &mut plan_wizard.constraints,
                        3 => &mut plan_wizard.priorities,
                        _ => &mut plan_wizard.notes,
                    };
                    target.pop();
                }
            }
            Action::PlanWizardSubmit => {
                let (follow_up, needs_clarification_question) =
                    if let AppState::Chat { plan_wizard, .. } = &self.state {
                        (
                            plan_helpers::build_plan_feedback_markdown(plan_wizard),
                            Self::plan_feedback_needs_clarification(plan_wizard),
                        )
                    } else {
                        (String::new(), false)
                    };
                if !follow_up.trim().is_empty() {
                    if let AppState::Chat {
                        command_input,
                        modal,
                        plan_waiting_for_clarification_question,
                        ..
                    } = &mut self.state
                    {
                        *modal = None;
                        command_input.set_text(follow_up);
                        *plan_waiting_for_clarification_question =
                            matches!(self.current_mode, TandemMode::Plan)
                                && needs_clarification_question;
                    }
                    self.sync_active_agent_from_chat();
                    if let Some(tx) = &self.action_tx {
                        let _ = tx.send(Action::SubmitCommand);
                    }
                }
                self.open_queued_plan_agent_prompt();
            }
            Action::RequestReject => {
                let (request_id, reject_kind, question_permission_id) = if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &self.state
                {
                    if let Some(request) = pending_requests.get(*request_cursor) {
                        match &request.kind {
                            PendingRequestKind::Permission(permission) => {
                                (Some(permission.id.clone()), Some("permission"), None)
                            }
                            PendingRequestKind::Question(question) => (
                                Some(question.id.clone()),
                                Some("question"),
                                question.permission_request_id.clone(),
                            ),
                        }
                    } else {
                        (None, None, None)
                    }
                } else {
                    (None, None, None)
                };
                if let (Some(request_id), Some(kind)) = (request_id, reject_kind) {
                    if let Some(client) = &self.client {
                        match kind {
                            "permission" => {
                                let _ = client.reply_permission(&request_id, "deny").await;
                            }
                            "question" => {
                                if let Some(permission_id) = question_permission_id {
                                    let _ = client.reply_permission(&permission_id, "deny").await;
                                }
                                let _ = client.reject_question(&request_id).await;
                            }
                            _ => {}
                        }
                    }
                    if let AppState::Chat {
                        pending_requests,
                        request_cursor,
                        modal,
                        ..
                    } = &mut self.state
                    {
                        pending_requests.retain(|request| match &request.kind {
                            PendingRequestKind::Permission(permission) => {
                                permission.id != request_id
                            }
                            PendingRequestKind::Question(question) => question.id != request_id,
                        });
                        if pending_requests.is_empty() {
                            *request_cursor = 0;
                            *modal = None;
                        } else if *request_cursor >= pending_requests.len() {
                            *request_cursor = pending_requests.len().saturating_sub(1);
                        }
                    }
                }
            }
            Action::RequestConfirm => {
                let mut remove_request_id: Option<String> = None;
                let mut permission_reply: Option<String> = None;
                let mut question_reply: Option<(String, Vec<Vec<String>>)> = None;
                let mut question_reply_preview: Option<String> = None;
                let mut question_permission_once: Option<String> = None;
                let mut approved_task_payload: Option<(String, Option<Value>)> = None;
                let mut approved_request_id: Option<String> = None;
                let mut question_request_target: Option<(String, String)> = None;

                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        let req_session_id = request.session_id.clone();
                        let req_agent_id = request.agent_id.clone();
                        match &mut request.kind {
                            PendingRequestKind::Permission(permission) => {
                                let reply = match *permission_choice {
                                    0 => "once",
                                    1 => "always",
                                    _ => "deny",
                                };
                                remove_request_id = Some(permission.id.clone());
                                permission_reply = Some(reply.to_string());
                                approved_request_id = Some(permission.id.clone());
                                if reply != "deny"
                                    && plan_helpers::is_task_tool_name(&permission.tool)
                                {
                                    approved_task_payload =
                                        Some((permission.tool.clone(), permission.args.clone()));
                                }
                            }
                            PendingRequestKind::Question(question) => {
                                let can_advance = if let Some(q) =
                                    question.questions.get_mut(question.question_index)
                                {
                                    // If the user highlighted an option but did not explicitly toggle
                                    // it, Enter should accept the highlighted choice.
                                    if q.selected_options.is_empty()
                                        && !q.options.is_empty()
                                        && q.option_cursor < q.options.len()
                                    {
                                        if q.multiple {
                                            q.selected_options.push(q.option_cursor);
                                        } else {
                                            q.selected_options = vec![q.option_cursor];
                                        }
                                    }
                                    !q.selected_options.is_empty()
                                        || !q.custom_input.trim().is_empty()
                                } else {
                                    false
                                };
                                if can_advance {
                                    if question.question_index + 1 < question.questions.len() {
                                        question.question_index += 1;
                                    } else {
                                        let mut answers: Vec<Vec<String>> = Vec::new();
                                        let mut answer_preview_lines: Vec<String> = Vec::new();
                                        for q in &question.questions {
                                            let mut question_answers = Vec::new();
                                            for idx in &q.selected_options {
                                                if let Some(option) = q.options.get(*idx) {
                                                    question_answers.push(option.label.clone());
                                                }
                                            }
                                            let custom = q.custom_input.trim();
                                            if !custom.is_empty() {
                                                question_answers.push(custom.to_string());
                                            }
                                            if question_answers.is_empty() {
                                                question_answers.push(String::new());
                                            }
                                            let preview_text = question_answers
                                                .iter()
                                                .filter(|s| !s.trim().is_empty())
                                                .cloned()
                                                .collect::<Vec<_>>()
                                                .join(" | ");
                                            answer_preview_lines.push(if preview_text.is_empty() {
                                                "- (empty)".to_string()
                                            } else {
                                                format!("- {}", preview_text)
                                            });
                                            answers.push(question_answers);
                                        }
                                        if !answer_preview_lines.is_empty() {
                                            question_reply_preview = Some(format!(
                                                "Submitted question answers:\n{}",
                                                answer_preview_lines.join("\n")
                                            ));
                                        }
                                        remove_request_id = Some(question.id.clone());
                                        if let Some(permission_id) =
                                            question.permission_request_id.clone()
                                        {
                                            question_permission_once = Some(permission_id);
                                        }
                                        question_reply = Some((question.id.clone(), answers));
                                        question_request_target =
                                            Some((req_session_id, req_agent_id));
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(client) = &self.client {
                    if let (Some(request_id), Some(reply)) =
                        (remove_request_id.clone(), permission_reply.clone())
                    {
                        let _ = client.reply_permission(&request_id, &reply).await;
                    }
                    if let Some(permission_id) = question_permission_once.clone() {
                        let _ = client.reply_permission(&permission_id, "once").await;
                    }
                    if let Some((question_id, answers)) = question_reply.clone() {
                        let _ = client.reply_question(&question_id, answers).await;
                    }
                }

                if let Some(request_id) = remove_request_id {
                    if permission_reply.is_some() || question_reply.is_some() {
                        if let AppState::Chat {
                            pending_requests,
                            request_cursor,
                            modal,
                            ..
                        } = &mut self.state
                        {
                            pending_requests.retain(|request| match &request.kind {
                                PendingRequestKind::Permission(permission) => {
                                    permission.id != request_id
                                }
                                PendingRequestKind::Question(question) => question.id != request_id,
                            });
                            if pending_requests.is_empty() {
                                *request_cursor = 0;
                                *modal = None;
                            } else if *request_cursor >= pending_requests.len() {
                                *request_cursor = pending_requests.len().saturating_sub(1);
                            }
                        }
                    }
                }
                if let Some(preview) = question_reply_preview {
                    if let AppState::Chat { messages, .. } = &mut self.state {
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(preview)],
                        });
                    }
                    self.sync_active_agent_from_chat();
                }

                if let Some((tool, args)) = approved_task_payload {
                    let fingerprint = plan_helpers::plan_fingerprint_from_args(args.as_ref());
                    let preview = plan_helpers::plan_preview_from_args(args.as_ref());
                    let should_open_wizard = if let AppState::Chat {
                        last_plan_task_fingerprint,
                        ..
                    } = &self.state
                    {
                        plan_helpers::is_todo_write_tool_name(&tool)
                            && !fingerprint.is_empty()
                            && *last_plan_task_fingerprint != fingerprint
                    } else {
                        false
                    };

                    if let AppState::Chat {
                        tasks,
                        active_task_id,
                        plan_wizard,
                        modal,
                        last_plan_task_fingerprint,
                        ..
                    } = &mut self.state
                    {
                        plan_helpers::apply_task_payload(
                            tasks,
                            active_task_id,
                            &tool,
                            args.as_ref(),
                        );
                        if plan_helpers::is_todo_write_tool_name(&tool) && !fingerprint.is_empty() {
                            *last_plan_task_fingerprint = fingerprint;
                        }
                        if should_open_wizard {
                            *modal = Some(ModalState::PlanFeedbackWizard);
                            *plan_wizard = PlanFeedbackWizardState {
                                plan_name: String::new(),
                                scope: String::new(),
                                constraints: String::new(),
                                priorities: String::new(),
                                notes: String::new(),
                                cursor_step: 0,
                                source_request_id: approved_request_id.clone(),
                                task_preview: preview,
                            };
                        }
                    }
                    if plan_helpers::is_todo_write_tool_name(&tool)
                        && matches!(self.current_mode, TandemMode::Plan)
                    {
                        self.queue_plan_agent_prompt(4);
                    }
                    self.sync_active_agent_from_chat();
                }

                if question_reply.is_some() && matches!(self.current_mode, TandemMode::Plan) {
                    if let Some((session_id, agent_id)) = question_request_target {
                        let follow_up = "Continue plan mode with the answered questions. Update `todowrite` tasks and statuses now, then ask for approval before execution.".to_string();
                        let mut queued = false;
                        if let AppState::Chat {
                            agents, messages, ..
                        } = &mut self.state
                        {
                            if let Some(agent) = agents
                                .iter_mut()
                                .find(|a| a.session_id == session_id && a.agent_id == agent_id)
                            {
                                if Self::is_agent_busy(&agent.status) {
                                    let merged_into_existing = !agent.follow_up_queue.is_empty();
                                    if merged_into_existing {
                                        if let Some(last) = agent.follow_up_queue.back_mut() {
                                            if !last.is_empty() {
                                                last.push('\n');
                                            }
                                            last.push_str(&follow_up);
                                        }
                                    } else {
                                        agent.follow_up_queue.push_back(follow_up.clone());
                                    }
                                    queued = true;
                                    if !merged_into_existing {
                                        messages.push(ChatMessage {
                                            role: MessageRole::System,
                                            content: vec![ContentBlock::Text(format!(
                                                "Queued follow-up message (#{}).",
                                                agent.follow_up_queue.len()
                                            ))],
                                        });
                                    }
                                }
                            }
                        }
                        if queued {
                            self.sync_active_agent_from_chat();
                        } else {
                            self.dispatch_prompt_for_agent(session_id, agent_id, follow_up);
                        }
                    }
                }
            }
            Action::NewAgent => {
                self.sync_active_agent_from_chat();
                let next_agent_id = if let AppState::Chat { agents, .. } = &self.state {
                    format!("A{}", agents.len() + 1)
                } else {
                    "A1".to_string()
                };
                let mut new_session_id: Option<String> = None;
                if let Some(client) = &self.client {
                    if let Ok(session) = client
                        .create_session(Some(format!("{} session", next_agent_id)))
                        .await
                    {
                        new_session_id = Some(session.id);
                    }
                }
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    let fallback_session = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    let pane = Self::make_agent_pane(
                        next_agent_id,
                        new_session_id.unwrap_or(fallback_session),
                    );
                    agents.push(pane);
                    *active_agent_index = agents.len().saturating_sub(1);
                }
                self.sync_chat_from_active_agent();
            }
            Action::CloseActiveAgent => {
                self.sync_active_agent_from_chat();
                let mut confirm = None;
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    modal,
                    ..
                } = &mut self.state
                {
                    if let Some(agent) = agents.get(*active_agent_index) {
                        if !agent.draft.text().trim().is_empty() {
                            confirm = Some(agent.agent_id.clone());
                        }
                    }
                    if let Some(agent_id) = confirm.clone() {
                        *modal = Some(ModalState::ConfirmCloseAgent {
                            target_agent_id: agent_id,
                        });
                    }
                }
                if confirm.is_none() {
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                    if let AppState::Chat {
                        agents,
                        modal,
                        active_agent_index,
                        grid_page,
                        ..
                    } = &mut self.state
                    {
                        if agents.len() <= 1 {
                            let replacement = Self::make_agent_pane(
                                "A1".to_string(),
                                agents
                                    .first()
                                    .map(|a| a.session_id.clone())
                                    .unwrap_or_default(),
                            );
                            agents.clear();
                            agents.push(replacement);
                        } else {
                            agents.remove(active_idx);
                            if *active_agent_index >= agents.len() {
                                *active_agent_index = agents.len().saturating_sub(1);
                            }
                            let max_page = agents.len().saturating_sub(1) / 4;
                            if *grid_page > max_page {
                                *grid_page = max_page;
                            }
                        }
                        *modal = None;
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::ConfirmCloseAgent(confirmed) => {
                if !confirmed {
                    if let AppState::Chat { modal, .. } = &mut self.state {
                        *modal = None;
                    }
                } else {
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                    if let AppState::Chat {
                        agents,
                        modal,
                        active_agent_index,
                        grid_page,
                        ..
                    } = &mut self.state
                    {
                        if agents.len() <= 1 {
                            let replacement = Self::make_agent_pane(
                                "A1".to_string(),
                                agents
                                    .first()
                                    .map(|a| a.session_id.clone())
                                    .unwrap_or_default(),
                            );
                            agents.clear();
                            agents.push(replacement);
                        } else {
                            agents.remove(active_idx);
                            if *active_agent_index >= agents.len() {
                                *active_agent_index = agents.len().saturating_sub(1);
                            }
                            let max_page = agents.len().saturating_sub(1) / 4;
                            if *grid_page > max_page {
                                *grid_page = max_page;
                            }
                        }
                        *modal = None;
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::ConfirmStartPlanAgents { confirmed, count } => {
                if let AppState::Chat {
                    modal,
                    plan_multi_agent_prompt,
                    ..
                } = &mut self.state
                {
                    *modal = None;
                    *plan_multi_agent_prompt = None;
                }
                if confirmed {
                    let created = self.ensure_agent_count(count).await;
                    if created > 0 {
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(format!(
                                    "Opened {} agent{} for plan execution.",
                                    created,
                                    if created == 1 { "" } else { "s" }
                                ))],
                            });
                        }
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::CancelActiveAgent => {
                let mut cancel_idx: Option<usize> = None;
                if let AppState::Chat {
                    modal,
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if modal.is_some() {
                        *modal = None;
                    } else if let Some(agent) = agents.get_mut(*active_agent_index) {
                        if matches!(agent.status, AgentStatus::Running | AgentStatus::Streaming) {
                            agent.status = AgentStatus::Cancelling;
                            cancel_idx = Some(*active_agent_index);
                        } else {
                            self.state = AppState::MainMenu;
                        }
                    }
                }
                if let Some(idx) = cancel_idx {
                    self.cancel_agent_if_running(idx).await;
                    if let AppState::Chat { agents, .. } = &mut self.state {
                        if let Some(agent) = agents.get_mut(idx) {
                            agent.status = AgentStatus::Idle;
                            agent.active_run_id = None;
                        }
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::StartDemoStream => {
                if let Some(tx) = &self.action_tx {
                    if let Some(agent) = self.active_agent_clone() {
                        let agent_id = agent.agent_id;
                        let session_id = agent.session_id;
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            let _ = tx.send(Action::PromptRunStarted {
                                session_id: session_id.clone(),
                                agent_id: agent_id.clone(),
                                run_id: Some(format!(
                                    "demo-{}",
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_millis())
                                        .unwrap_or(0)
                                )),
                            });
                            let tokens = ["demo ", "stream ", "for ", "active ", "agent"];
                            for t in tokens {
                                let _ = tx.send(Action::PromptDelta {
                                    session_id: session_id.clone(),
                                    agent_id: agent_id.clone(),
                                    delta: t.to_string(),
                                });
                                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                            }
                        });
                    }
                }
            }
            Action::SpawnBackgroundDemo => {
                self.sync_active_agent_from_chat();
                let previous_active = if let AppState::Chat {
                    active_agent_index, ..
                } = &self.state
                {
                    *active_agent_index
                } else {
                    0
                };
                let next_agent_id = if let AppState::Chat { agents, .. } = &self.state {
                    format!("A{}", agents.len() + 1)
                } else {
                    "A1".to_string()
                };
                let mut new_session_id: Option<String> = None;
                if let Some(client) = &self.client {
                    if let Ok(session) = client
                        .create_session(Some(format!("{} session", next_agent_id)))
                        .await
                    {
                        new_session_id = Some(session.id);
                    }
                }
                let (new_agent_id, new_agent_session_id) = if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    let fallback_session = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    let pane = Self::make_agent_pane(
                        next_agent_id.clone(),
                        new_session_id.unwrap_or(fallback_session),
                    );
                    agents.push(pane);
                    *active_agent_index = agents.len().saturating_sub(1);
                    let session_id = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    (next_agent_id, session_id)
                } else {
                    ("A1".to_string(), String::new())
                };
                self.sync_chat_from_active_agent();
                if let Some(tx) = &self.action_tx {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(Action::PromptRunStarted {
                            session_id: new_agent_session_id.clone(),
                            agent_id: new_agent_id.clone(),
                            run_id: Some(format!(
                                "demo-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0)
                            )),
                        });
                        let tokens = ["background ", "demo ", "stream"];
                        for t in tokens {
                            let _ = tx.send(Action::PromptDelta {
                                session_id: new_agent_session_id.clone(),
                                agent_id: new_agent_id.clone(),
                                delta: t.to_string(),
                            });
                            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                        }
                    });
                }
                if let AppState::Chat {
                    active_agent_index,
                    agents,
                    ..
                } = &mut self.state
                {
                    *active_agent_index = previous_active.min(agents.len().saturating_sub(1));
                }
                self.sync_chat_from_active_agent();
            }

            Action::SetupNextStep => {
                let mut persist_provider: Option<(String, Option<String>, Option<String>)> = None;
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    api_key_input,
                    model_input,
                } = &mut self.state
                {
                    match step.clone() {
                        SetupStep::Welcome => {
                            *step = SetupStep::SelectProvider;
                        }
                        SetupStep::SelectProvider => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    *step = SetupStep::EnterApiKey;
                                }
                            } else {
                                *step = SetupStep::EnterApiKey;
                            }
                            model_input.clear();
                        }
                        SetupStep::EnterApiKey => {
                            if !api_key_input.is_empty() {
                                *step = SetupStep::SelectModel;
                            }
                        }
                        SetupStep::SelectModel => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    let provider = &catalog.all[*selected_provider_index];
                                    let model_ids = Self::filtered_model_ids(
                                        catalog,
                                        *selected_provider_index,
                                        model_input,
                                    );
                                    let model_id = if model_ids.is_empty() {
                                        if model_input.trim().is_empty() {
                                            None
                                        } else {
                                            Some(model_input.trim().to_string())
                                        }
                                    } else {
                                        model_ids.get(*selected_model_index).cloned()
                                    };
                                    let api_key = if api_key_input.is_empty() {
                                        None
                                    } else {
                                        Some(api_key_input.clone())
                                    };
                                    persist_provider =
                                        Some((provider.id.clone(), model_id, api_key));
                                }
                            }
                            *step = SetupStep::Complete;
                        }
                        SetupStep::Complete => {
                            // Transition to MainMenu or Chat
                            self.state = AppState::MainMenu;
                        }
                    }
                }
                if let Some((provider_id, model_id, api_key)) = persist_provider {
                    self.current_provider = Some(provider_id.clone());
                    self.current_model = model_id.clone();
                    if let Some(ref key) = api_key {
                        self.save_provider_key_local(&provider_id, key);
                    }
                    self.persist_provider_defaults(
                        &provider_id,
                        model_id.as_deref(),
                        api_key.as_deref(),
                    )
                    .await;
                }
            }

            Action::SetupPrevItem => {
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    model_input,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::SelectProvider => {
                            if *selected_provider_index > 0 {
                                *selected_provider_index -= 1;
                            }
                            *selected_model_index = 0;
                            model_input.clear();
                        }
                        SetupStep::SelectModel => {
                            *selected_model_index = 0;
                        }
                        _ => {}
                    }

                    if let Some(catalog) = provider_catalog {
                        if *selected_provider_index >= catalog.all.len() {
                            *selected_provider_index = catalog.all.len().saturating_sub(1);
                        }
                    }
                }
            }
            Action::SetupNextItem => {
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    model_input,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::SelectProvider => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() - 1 {
                                    *selected_provider_index += 1;
                                }
                            }
                            model_input.clear();
                        }
                        SetupStep::SelectModel => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    let model_ids = Self::filtered_model_ids(
                                        catalog,
                                        *selected_provider_index,
                                        model_input,
                                    );
                                    if !model_ids.is_empty()
                                        && *selected_model_index < model_ids.len() - 1
                                    {
                                        *selected_model_index += 1;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            Action::SetupInput(c) => {
                if let AppState::SetupWizard {
                    step,
                    api_key_input,
                    model_input,
                    selected_model_index,
                    provider_catalog,
                    selected_provider_index,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::EnterApiKey => {
                            api_key_input.push(c);
                        }
                        SetupStep::SelectModel => {
                            model_input.push(c);
                            if let Some(catalog) = provider_catalog {
                                let model_count = Self::filtered_model_ids(
                                    catalog,
                                    *selected_provider_index,
                                    model_input,
                                )
                                .len();
                                if model_count == 0 {
                                    *selected_model_index = 0;
                                } else if *selected_model_index >= model_count {
                                    *selected_model_index = model_count.saturating_sub(1);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            Action::SetupBackspace => {
                if let AppState::SetupWizard {
                    step,
                    api_key_input,
                    model_input,
                    selected_model_index,
                    provider_catalog,
                    selected_provider_index,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::EnterApiKey => {
                            api_key_input.pop();
                        }
                        SetupStep::SelectModel => {
                            model_input.pop();
                            if let Some(catalog) = provider_catalog {
                                let model_count = Self::filtered_model_ids(
                                    catalog,
                                    *selected_provider_index,
                                    model_input,
                                )
                                .len();
                                if model_count == 0 {
                                    *selected_model_index = 0;
                                } else if *selected_model_index >= model_count {
                                    *selected_model_index = model_count.saturating_sub(1);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            Action::OverlayScrollUp => {
                if let Some(overlay) = &mut self.pager_overlay {
                    overlay.scroll = overlay.scroll.saturating_sub(1);
                }
            }
            Action::OverlayScrollDown => {
                if let Some(overlay) = &mut self.pager_overlay {
                    overlay.scroll = overlay.scroll.saturating_add(1);
                }
            }
            Action::OverlayPageUp => {
                if let Some(overlay) = &mut self.pager_overlay {
                    overlay.scroll = overlay.scroll.saturating_sub(SCROLL_PAGE_STEP as usize);
                }
            }
            Action::OverlayPageDown => {
                if let Some(overlay) = &mut self.pager_overlay {
                    overlay.scroll = overlay.scroll.saturating_add(SCROLL_PAGE_STEP as usize);
                }
            }
            Action::ScrollUp => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_add(SCROLL_LINE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::ScrollDown => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_sub(SCROLL_LINE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::PageUp => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_add(SCROLL_PAGE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::PageDown => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_sub(SCROLL_PAGE_STEP);
                }
                self.sync_active_agent_from_chat();
            }

            Action::ClearCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.clear();
                }
                self.sync_active_agent_from_chat();
            }

            Action::QueueSteeringFromComposer => {
                let mut queue_note: Option<String> = None;
                let mut queue_error: Option<String> = None;
                let mut should_cancel_active = false;
                let mut should_dispatch_now = false;
                if let AppState::Chat {
                    command_input,
                    agents,
                    active_agent_index,
                    messages,
                    ..
                } = &mut self.state
                {
                    let raw = command_input.text().to_string();
                    if raw.trim().is_empty() {
                        return Ok(());
                    }
                    let msg = raw.trim().to_string();
                    if let Some(agent) = agents.get_mut(*active_agent_index) {
                        match Self::expand_paste_markers_checked(&msg, agent) {
                            Ok(expanded) => {
                                command_input.clear();
                                if Self::is_agent_busy(&agent.status) {
                                    agent.steering_message = Some(expanded);
                                    agent.follow_up_queue.clear();
                                    should_cancel_active = agent.active_run_id.is_some();
                                    queue_note = Some(
                                        "Steering message queued. Current run will be interrupted."
                                            .to_string(),
                                    );
                                } else {
                                    command_input.set_text(expanded);
                                    should_dispatch_now = true;
                                }
                            }
                            Err(err) => queue_error = Some(err),
                        }
                    }
                    if let Some(err) = queue_error.clone() {
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(err)],
                        });
                    }
                    if let Some(note) = queue_note.clone() {
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(note)],
                        });
                    }
                }
                self.sync_active_agent_from_chat();
                if should_cancel_active {
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                }
                if should_dispatch_now {
                    if let Some(tx) = &self.action_tx {
                        let _ = tx.send(Action::SubmitCommand);
                    }
                }
            }

            Action::SubmitCommand => {
                let (session_id, active_agent_id, msg_to_send, queued_followup) =
                    if let AppState::Chat {
                        session_id,
                        command_input,
                        agents,
                        active_agent_index,
                        plan_awaiting_approval,
                        messages,
                        ..
                    } = &mut self.state
                    {
                        let raw = command_input.text().to_string();
                        if raw.trim().is_empty() {
                            return Ok(());
                        }
                        let msg = raw.trim().to_string();
                        let mut agent_id = "A1".to_string();
                        let mut queued = false;
                        let mut msg_to_send: Option<String> = None;
                        let mut blocked_error: Option<String> = None;
                        if let Some(agent) = agents.get_mut(*active_agent_index) {
                            agent_id = agent.agent_id.clone();
                            match Self::expand_paste_markers_checked(&msg, agent) {
                                Ok(expanded) => {
                                    command_input.clear();
                                    if Self::is_agent_busy(&agent.status) {
                                        let merged_into_existing =
                                            !agent.follow_up_queue.is_empty();
                                        if merged_into_existing {
                                            if let Some(last) = agent.follow_up_queue.back_mut() {
                                                if !last.is_empty() {
                                                    last.push('\n');
                                                }
                                                last.push_str(&expanded);
                                            }
                                        } else {
                                            agent.follow_up_queue.push_back(expanded);
                                        }
                                        queued = true;
                                        if !merged_into_existing {
                                            messages.push(ChatMessage {
                                                role: MessageRole::System,
                                                content: vec![ContentBlock::Text(format!(
                                                    "Queued follow-up message (#{}).",
                                                    agent.follow_up_queue.len()
                                                ))],
                                            });
                                        }
                                    } else {
                                        msg_to_send = Some(expanded);
                                    }
                                }
                                Err(err) => {
                                    blocked_error = Some(err);
                                    command_input.set_text(msg.clone());
                                }
                            }
                        }
                        if let Some(err) = blocked_error {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(err)],
                            });
                        }
                        *plan_awaiting_approval = false;
                        (session_id.clone(), agent_id, msg_to_send, queued)
                    } else {
                        (String::new(), "A1".to_string(), None, false)
                    };
                if queued_followup {
                    self.sync_active_agent_from_chat();
                    return Ok(());
                }

                if let Some(msg) = msg_to_send {
                    let is_single_line = !msg.contains('\n');
                    if is_single_line && msg.starts_with("/tool ") {
                        // Pass through engine-native tool invocation syntax.
                        // The engine loop handles permission and execution for /tool.
                        self.dispatch_prompt_for_agent(
                            session_id.clone(),
                            active_agent_id.clone(),
                            msg.clone(),
                        );
                    } else if is_single_line && msg.starts_with('/') {
                        let response = self.execute_command(&msg).await;
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(response)],
                            });
                        }
                        self.sync_active_agent_from_chat();
                    } else if let Some(provider_id) = self.pending_model_provider.clone() {
                        let model_id = msg.trim().to_string();
                        if model_id.is_empty() {
                            if let AppState::Chat { messages, .. } = &mut self.state {
                                messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: vec![ContentBlock::Text(
                                        "Model cannot be empty. Paste a model name.".to_string(),
                                    )],
                                });
                            }
                        } else {
                            self.pending_model_provider = None;
                            self.current_provider = Some(provider_id.clone());
                            self.current_model = Some(model_id.clone());
                            self.persist_provider_defaults(&provider_id, Some(&model_id), None)
                                .await;
                            if let AppState::Chat { messages, .. } = &mut self.state {
                                messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: vec![ContentBlock::Text(format!(
                                        "Provider set to {} with model {}.",
                                        provider_id, model_id
                                    ))],
                                });
                            }
                            self.sync_active_agent_from_chat();
                        }
                    } else {
                        if let Some(provider_id) = self.current_provider.clone() {
                            if !self.provider_is_connected(&provider_id)
                                && self.open_key_wizard_for_provider(&provider_id)
                            {
                                if let AppState::Chat { messages, .. } = &mut self.state {
                                    messages.push(ChatMessage {
                                        role: MessageRole::System,
                                        content: vec![ContentBlock::Text(format!(
                                            "Provider '{}' has no configured key. Enter API key in setup wizard to continue.",
                                            provider_id
                                        ))],
                                    });
                                }
                                self.sync_active_agent_from_chat();
                                return Ok(());
                            }
                        }
                        self.dispatch_prompt_for_agent(
                            session_id.clone(),
                            active_agent_id.clone(),
                            msg.clone(),
                        );
                    }
                }
            }

            Action::PromptRunStarted {
                session_id: event_session_id,
                agent_id,
                run_id,
            } => self.handle_prompt_run_started(event_session_id, agent_id, run_id),
            Action::PromptSuccess {
                session_id: event_session_id,
                agent_id,
                messages: new_messages,
            } => self.handle_prompt_success(event_session_id, agent_id, new_messages),
            Action::PromptTodoUpdated {
                session_id: event_session_id,
                todos,
            } => {
                self.handle_prompt_todo_updated(event_session_id, todos)
                    .await
            }
            Action::PromptAgentTeamEvent {
                session_id: event_session_id,
                agent_id,
                event,
            } => {
                self.handle_prompt_agent_team_event(event_session_id, agent_id, event)
                    .await
            }
            Action::PromptDelta {
                session_id: event_session_id,
                agent_id,
                delta,
            } => self.handle_prompt_delta(event_session_id, agent_id, delta),
            Action::PromptInfo {
                session_id: event_session_id,
                agent_id,
                message,
            } => self.handle_prompt_info(event_session_id, agent_id, message),
            Action::PromptToolDelta {
                session_id: event_session_id,
                agent_id,
                tool_call_id,
                tool_name,
                args_delta: _,
                args_preview,
            } => self.handle_prompt_tool_delta(
                event_session_id,
                agent_id,
                tool_call_id,
                tool_name,
                args_preview,
            ),
            Action::PromptMalformedQuestion {
                session_id: event_session_id,
                agent_id,
                request_id,
            } => {
                self.handle_prompt_malformed_question(event_session_id, agent_id, request_id)
                    .await
            }
            Action::PromptRequest {
                session_id: event_session_id,
                agent_id,
                request,
            } => {
                self.handle_prompt_request(event_session_id, agent_id, request)
                    .await
            }
            Action::PromptRequestResolved { request_id, .. } => {
                self.handle_prompt_request_resolved(request_id)
            }
            Action::PromptFailure {
                session_id: event_session_id,
                agent_id,
                error,
            } => self.handle_prompt_failure(event_session_id, agent_id, error),

            _ => {}
}
