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
