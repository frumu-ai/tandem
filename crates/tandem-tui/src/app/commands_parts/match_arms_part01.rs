        "help" => Some(HELP_TEXT.to_string()),
        "diff" => Some(app.open_diff_overlay().await),
        "files" => {
            let query = if args.is_empty() {
                None
            } else {
                Some(args.join(" "))
            };
            app.open_file_search_modal(query.as_deref());
            Some(if let Some(q) = query {
                format!("Opened file search for query: {}", q)
            } else {
                "Opened file search overlay.".to_string()
            })
        }
        "edit" => Some(app.open_external_editor_for_active_input().await),
        "workspace" => Some(match args.first().copied() {
            Some("show") | None => {
                let cwd = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "<unknown>".to_string());
                format!("Current workspace directory:\n  {}", cwd)
            }
            Some("use") => {
                let raw_path = args
                    .get(1..)
                    .map(|items| items.join(" "))
                    .unwrap_or_default();
                if raw_path.trim().is_empty() {
                    return Some("Usage: /workspace use <path>".to_string());
                }
                let target = match App::resolve_workspace_path(raw_path.trim()) {
                    Ok(path) => path,
                    Err(err) => return Some(err),
                };
                let previous = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "<unknown>".to_string());
                if let Err(err) = std::env::set_current_dir(&target) {
                    return Some(format!(
                        "Failed to switch workspace to {}: {}",
                        target.display(),
                        err
                    ));
                }
                let current = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| target.display().to_string());
                format!(
                    "Workspace switched.\n  From: {}\n  To:   {}",
                    previous, current
                )
            }
            _ => "Usage: /workspace show | use <path>".to_string(),
        }),
        "engine" => Some(match args.first().copied() {
            Some("status") => {
                if let Some(client) = &app.client {
                    match client.get_engine_status().await {
                        Ok(status) => {
                            let required = App::desired_engine_version()
                                .map(App::format_semver_triplet)
                                .unwrap_or_else(|| "unknown".to_string());
                            let stale_policy = EngineStalePolicy::from_env();
                            format!(
                                "Engine Status:\n  Healthy: {}\n  Version: {}\n  Required: {}\n  Mode: {}\n  Endpoint: {}\n  Source: {}\n  Stale policy: {}",
                                if status.healthy { "Yes" } else { "No" },
                                status.version,
                                required,
                                status.mode,
                                client.base_url(),
                                app.engine_connection_source.as_str(),
                                stale_policy.as_str()
                            )
                        }
                        Err(e) => format!("Failed to get engine status: {}", e),
                    }
                } else {
                    "Engine: Not connected".to_string()
                }
            }
            Some("restart") => {
                app.connection_status = "Restarting engine...".to_string();
                app.release_engine_lease().await;
                app.stop_engine_process().await;
                app.client = None;
                app.engine_base_url_override = None;
                app.engine_connection_source = EngineConnectionSource::Unknown;
                app.engine_spawned_at = None;
                app.provider_catalog = None;
                sleep(std::time::Duration::from_millis(300)).await;
                app.state = AppState::Connecting;
                "Engine restart requested.".to_string()
            }
            Some("token") => {
                let show_full = args.get(1).map(|s| s.eq_ignore_ascii_case("show")) == Some(true);
                let Some(token) = app.engine_api_token.as_deref().map(str::trim) else {
                    return Some("Engine token is not configured.".to_string());
                };
                if token.is_empty() {
                    return Some("Engine token is not configured.".to_string());
                }
                let value = if show_full {
                    token.to_string()
                } else {
                    App::masked_engine_api_token(token)
                };
                let path = engine_api_token_file_path().to_string_lossy().to_string();
                let backend = app
                    .engine_api_token_backend
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                if show_full {
                    format!(
                        "Engine API token:\n  {}\nStorage: {}\nPath:\n  {}",
                        value, backend, path
                    )
                } else {
                    format!(
                        "Engine API token (masked):\n  {}\nStorage: {}\nUse `/engine token show` to reveal.\nPath:\n  {}",
                        value, backend, path
                    )
                }
            }
            _ => "Usage: /engine status | restart | token [show]".to_string(),
        }),
        "browser" => Some(match args.first().copied() {
            Some("status") | Some("doctor") => {
                if let Some(client) = &app.client {
                    match client.get_browser_status().await {
                        Ok(status) => {
                            let mut lines = vec![
                                "Browser Status:".to_string(),
                                format!("  Enabled: {}", if status.enabled { "Yes" } else { "No" }),
                                format!(
                                    "  Runnable: {}",
                                    if status.runnable { "Yes" } else { "No" }
                                ),
                                format!(
                                    "  Sidecar: {}",
                                    status
                                        .sidecar
                                        .path
                                        .clone()
                                        .unwrap_or_else(|| "<not found>".to_string())
                                ),
                                format!(
                                    "  Browser: {}",
                                    status
                                        .browser
                                        .path
                                        .clone()
                                        .unwrap_or_else(|| "<not found>".to_string())
                                ),
                            ];
                            if let Some(version) = status.browser.version.as_deref() {
                                lines.push(format!("  Browser version: {}", version));
                            }
                            if !status.blocking_issues.is_empty() {
                                lines.push("Blocking issues:".to_string());
                                for issue in status.blocking_issues {
                                    lines.push(format!("  - {}: {}", issue.code, issue.message));
                                }
                            }
                            if !status.recommendations.is_empty() {
                                lines.push("Recommendations:".to_string());
                                for row in status.recommendations {
                                    lines.push(format!("  - {}", row));
                                }
                            }
                            if !status.install_hints.is_empty() {
                                lines.push("Install hints:".to_string());
                                for row in status.install_hints {
                                    lines.push(format!("  - {}", row));
                                }
                            }
                            lines.join("\n")
                        }
                        Err(e) => format!("Failed to get browser status: {}", e),
                    }
                } else {
                    "Engine: Not connected".to_string()
                }
            }
            _ => "Usage: /browser status | doctor".to_string(),
        }),
        "agent" => Some(match args.first().copied() {
            Some("new") => {
                app.sync_active_agent_from_chat();
                let next_agent_id = if let AppState::Chat { agents, .. } = &app.state {
                    format!("A{}", agents.len() + 1)
                } else {
                    "A1".to_string()
                };
                let mut new_session_id: Option<String> = None;
                if let Some(client) = &app.client {
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
                } = &mut app.state
                {
                    let fallback_session = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    let pane = App::make_agent_pane(
                        next_agent_id,
                        new_session_id.unwrap_or(fallback_session),
                    );
                    agents.push(pane);
                    *active_agent_index = agents.len().saturating_sub(1);
                }
                app.sync_chat_from_active_agent();
                "Created new agent.".to_string()
            }
            Some("list") => {
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &app.state
                {
                    let mut out = Vec::new();
                    for (i, a) in agents.iter().enumerate() {
                        let marker = if i == *active_agent_index { ">" } else { " " };
                        out.push(format!(
                            "{} {} [{}] {}",
                            marker,
                            a.agent_id,
                            a.session_id,
                            format!("{:?}", a.status)
                        ));
                    }
                    format!("Agents:\n{}", out.join("\n"))
                } else {
                    "Not in chat.".to_string()
                }
            }
            Some("use") => {
                if let Some(agent_id) = args.get(1) {
                    app.sync_active_agent_from_chat();
                    if let AppState::Chat {
                        agents,
                        active_agent_index,
                        ..
                    } = &mut app.state
                    {
                        if let Some(idx) = agents.iter().position(|a| &a.agent_id == agent_id) {
                            *active_agent_index = idx;
                            app.sync_chat_from_active_agent();
                            return Some(format!("Switched to {}.", agent_id));
                        }
                    }
                    format!("Agent not found: {}", agent_id)
                } else {
                    "Usage: /agent use <A#>".to_string()
                }
            }
            Some("close") => {
                app.sync_active_agent_from_chat();
                let active_idx = if let AppState::Chat {
                    active_agent_index, ..
                } = &app.state
                {
                    *active_agent_index
                } else {
                    0
                };
                app.cancel_agent_if_running(active_idx).await;
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    grid_page,
                    ..
                } = &mut app.state
                {
                    if agents.len() <= 1 {
                        return Some("Cannot close last agent.".to_string());
                    }
                    agents.remove(active_idx);
                    if *active_agent_index >= agents.len() {
                        *active_agent_index = agents.len().saturating_sub(1);
                    }
                    let max_page = agents.len().saturating_sub(1) / 4;
                    if *grid_page > max_page {
                        *grid_page = max_page;
                    }
                }
                app.sync_chat_from_active_agent();
                "Closed active agent.".to_string()
            }
            Some("fanout") => {
                let mode_switched = if matches!(app.current_mode, TandemMode::Plan) {
                    app.current_mode = TandemMode::Orchestrate;
                    true
                } else {
                    false
                };
                let mode_note = if mode_switched {
                    " Mode auto-switched from plan -> orchestrate."
                } else {
                    ""
                };
                let (target, goal_start_idx) = match args.get(1) {
                    Some(raw) => match raw.parse::<usize>() {
                        Ok(n) => (n.clamp(2, 9), 2),
                        Err(_) => (4, 1),
                    },
                    None => (4, 1),
                };
                let goal = args
                    .iter()
                    .skip(goal_start_idx)
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string();
                let created = app.ensure_agent_count(target).await;
                if let AppState::Chat {
                    ui_mode, grid_page, ..
                } = &mut app.state
                {
                    *ui_mode = UiMode::Grid;
                    *grid_page = 0;
                }
                app.sync_chat_from_active_agent();
                if !goal.is_empty() {
                    let agents = if let AppState::Chat { agents, .. } = &app.state {
                        agents.iter().take(target).cloned().collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    };
                    if let Some(lead) = agents.first() {
                        let team_name = format!(
                            "fanout-{}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0)
                        );
                        let create_team_args = serde_json::json!({
                            "team_name": team_name,
                            "description": format!("Fanout run for goal: {}", goal),
                            "agent_type": "lead"
                        });
                        let mut lead_commands =
                            vec![format!("/tool TeamCreate {}", create_team_args)];
                        for agent in agents.iter().skip(1) {
                            let task_prompt = format!(
                                "You are {} in a coordinated fanout run for team `{}`.\n\
                                 Goal: {}.\n\
                                 Own one concrete workstream end-to-end, execute it, and report concise outcomes and blockers.\n\
                                 Do not ask clarification questions unless absolutely blocked.\n\
                                 Do not wait for plan approvals; make reasonable assumptions and proceed.",
                                agent.agent_id, team_name, goal
                            );
                            let task_args = serde_json::json!({
                                "description": format!("{} workstream for {}", agent.agent_id, goal),
                                "prompt": task_prompt,
                                "subagent_type": "generalist",
                                "team_name": team_name,
                                "name": agent.agent_id
                            });
                            lead_commands.push(format!("/tool task {}", task_args));
                        }
                        let lead_kickoff = format!(
                            "You are the lead coordinator for team `{}`. Goal: {}.\n\
                             Use TaskList/TaskUpdate to track delegated progress and keep execution moving until completion.",
                            team_name, goal
                        );
                        lead_commands.push(lead_kickoff);
                        if let AppState::Chat { agents, .. } = &mut app.state {
                            if let Some(lead_agent) = agents.iter_mut().find(|a| {
                                a.agent_id == lead.agent_id && a.session_id == lead.session_id
                            }) {
                                for cmd in lead_commands {
                                    lead_agent.follow_up_queue.push_back(cmd);
                                }
                            }
                        }
                        app.maybe_dispatch_queued_for_agent(&lead.session_id, &lead.agent_id);
                        return Some(format!(
                            "Started coordinated fanout: {} total agents (created {}). Team `{}` bootstrapped and assignments dispatched.{}",
                            target, created, team_name, mode_note
                        ));
                    }
                    return Some(format!(
                        "Started coordinated fanout: {} total agents (created {}). Goal dispatched.{}",
                        target, created, mode_note
                    ));
                }
                if created > 0 {
                    format!(
                        "Started fanout: {} total agents (created {}). Grid view enabled.{}",
                        target, created, mode_note
                    )
                } else {
                    format!(
                        "Fanout ready: already at {}+ agents. Grid view enabled.{}",
                        target, mode_note
                    )
                }
            }
            _ => "Usage: /agent new|list|use <A#>|close|fanout [n] [goal]".to_string(),
        }),
        "sessions" => Some(if app.sessions.is_empty() {
            "No sessions found.".to_string()
        } else {
            let lines: Vec<String> = app
                .sessions
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let marker = if i == app.selected_session_index {
                        "→ "
                    } else {
                        "  "
                    };
                    format!("{}{} (ID: {})", marker, s.title, s.id)
                })
                .collect();
            format!("Sessions:\n{}", lines.join("\n"))
        }),
        "new" => Some({
            let title = if args.is_empty() {
                None
            } else {
                Some(args.join(" ").trim().to_string())
            };
            let title_for_display = title.clone().unwrap_or_else(|| "New Session".to_string());
            if let Some(client) = &app.client {
                match client.create_session(title).await {
                    Ok(session) => {
                        app.sessions.push(session.clone());
                        app.selected_session_index = app.sessions.len() - 1;
                        format!(
                            "Created session: {} (ID: {})",
                            title_for_display, session.id
                        )
                    }
                    Err(e) => format!("Failed to create session: {}", e),
                }
            } else {
                "Not connected to engine".to_string()
            }
        }),
        "recent" => Some(match args.first().copied() {
            Some("run") => {
                let Some(raw_index) = args.get(1) else {
                    return Some("Usage: /recent run <index>".to_string());
                };
                let Ok(index) = raw_index.parse::<usize>() else {
                    return Some(format!("Invalid recent-command index: {}", raw_index));
                };
                if index == 0 {
                    return Some("Recent-command index is 1-based.".to_string());
                }
                let commands = app.recent_commands_snapshot();
                let Some(command) = commands.get(index - 1).cloned() else {
                    return Some(format!(
                        "Recent-command index {} is out of range ({} stored).",
                        index,
                        commands.len()
                    ));
                };
                let result = Box::pin(app.execute_command(&command)).await;
                format!(
                    "Replayed recent command #{}: {}\n\n{}",
                    index, command, result
                )
            }
            Some("clear") => {
                let cleared = app.clear_recent_commands();
                format!("Cleared {} recent command(s).", cleared)
            }
            Some("list") | None => {
                let commands = app.recent_commands_snapshot();
                if commands.is_empty() {
                    "No recent slash commands yet.".to_string()
                } else {
                    format!(
                        "Recent commands:\n{}\n\nNext\n  /recent run <index>\n  /recent clear",
                        commands
                            .iter()
                            .enumerate()
                            .map(|(idx, command)| format!("  {}. {}", idx + 1, command))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                }
            }
            _ => "Usage: /recent [list|run <index>|clear]".to_string(),
        }),
        "use" => Some({
            let Some(target_id) = args.first().copied() else {
                return Some("Usage: /use <session_id>".to_string());
            };
            if let Some(idx) = app.sessions.iter().position(|s| s.id == target_id) {
                app.selected_session_index = idx;
                let loaded_messages = app.load_chat_history(target_id).await;
                let (recalled_tasks, recalled_active_task_id) =
                    plan_helpers::rebuild_tasks_from_messages(&loaded_messages);
                if let AppState::Chat {
                    session_id,
                    messages,
                    scroll_from_bottom,
                    tasks,
                    active_task_id,
                    agents,
                    active_agent_index,
                    ..
                } = &mut app.state
                {
                    *session_id = target_id.to_string();
                    *messages = loaded_messages.clone();
                    *scroll_from_bottom = 0;
                    *tasks = recalled_tasks.clone();
                    *active_task_id = recalled_active_task_id.clone();
                    if let Some(agent) = agents.get_mut(*active_agent_index) {
                        agent.session_id = target_id.to_string();
                        agent.messages = loaded_messages;
                        agent.scroll_from_bottom = 0;
                        agent.tasks = recalled_tasks;
                        agent.active_task_id = recalled_active_task_id;
                    }
                }
                format!("Switched to session: {}", target_id)
            } else {
                format!("Session not found: {}", target_id)
            }
        }),
        "keys" => Some(if let Some(keystore) = &app.keystore {
            let mut provider_ids: Vec<String> = keystore
                .list_keys()
                .into_iter()
                .map(|k| App::normalize_provider_id_from_keystore_key(&k))
                .collect();
            provider_ids.sort();
            provider_ids.dedup();
            if provider_ids.is_empty() {
                "No provider keys configured.".to_string()
            } else {
                format!(
                    "Configured providers:\n{}",
                    provider_ids
                        .iter()
                        .map(|p| format!("  {} - configured", p))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        } else {
            "Keystore not unlocked. Enter PIN to access keys.".to_string()
        }),
        "key" => Some(match args.first().copied() {
            Some("set") => {
                let provider_id = args
                    .get(1)
                    .map(|s| s.to_string())
                    .or_else(|| app.current_provider.clone());
                let Some(provider_id) = provider_id else {
                    return Some(
                        "Usage: /key set <provider_id> (or set /provider first)".to_string(),
                    );
                };
                if app.open_key_wizard_for_provider(&provider_id) {
                    format!("Opening key setup wizard for {}...", provider_id)
                } else {
                    format!("Provider not found: {}", provider_id)
                }
            }
            Some("remove") => {
                let Some(provider_id) = args.get(1).copied() else {
                    return Some("Usage: /key remove <provider_id>".to_string());
                };
                format!("Key removal not implemented. Provider: {}", provider_id)
            }
            Some("test") => {
                let Some(provider_id) = args.get(1).copied() else {
                    return Some("Usage: /key test <provider_id>".to_string());
                };
                if let Some(client) = &app.client {
                    if let Ok(catalog) = client.list_providers().await {
                        let catalog = App::sanitize_provider_catalog(catalog);
                        let is_connected = catalog.connected.contains(&provider_id.to_string());
                        if catalog.all.iter().any(|p| p.id == provider_id) {
                            if is_connected {
                                return Some(format!(
                                    "Provider {}: Connected and working!",
                                    provider_id
                                ));
                            }
                            return Some(format!(
                                "Provider {}: Not connected. Use /key set to add credentials.",
                                provider_id
                            ));
                        }
                    }
                }
                format!("Provider {}: Not connected or not available.", provider_id)
            }
            _ => "Usage: /key set|remove|test <provider_id>".to_string(),
        }),
        "cancel" => Some({
            let active_idx = if let AppState::Chat {
                active_agent_index, ..
            } = &app.state
            {
                *active_agent_index
            } else {
                0
            };
            app.cancel_agent_if_running(active_idx).await;
            if let AppState::Chat { agents, .. } = &mut app.state {
                if let Some(agent) = agents.get_mut(active_idx) {
                    agent.status = AgentStatus::Idle;
                    agent.active_run_id = None;
                }
            }
            app.sync_chat_from_active_agent();
            "Cancel requested for active agent.".to_string()
        }),
        "steer" => Some({
            if args.is_empty() {
                return Some("Usage: /steer <message>".to_string());
            }
            let msg = args.join(" ");
            if let AppState::Chat { command_input, .. } = &mut app.state {
                command_input.set_text(msg);
            }
            if let Some(tx) = &app.action_tx {
                let _ = tx.send(Action::QueueSteeringFromComposer);
            }
            "Steering message queued.".to_string()
        }),
        "followup" => Some({
            if args.is_empty() {
                return Some("Usage: /followup <message>".to_string());
            }
            let msg = args.join(" ");
            let mut queued_len = 0usize;
            if let AppState::Chat {
                agents,
                active_agent_index,
                ..
            } = &mut app.state
            {
                if let Some(agent) = agents.get_mut(*active_agent_index) {
                    let merged_into_existing = !agent.follow_up_queue.is_empty();
                    if merged_into_existing {
                        if let Some(last) = agent.follow_up_queue.back_mut() {
                            if !last.is_empty() {
                                last.push('\n');
                            }
                            last.push_str(&msg);
                        }
                    } else {
                        agent.follow_up_queue.push_back(msg);
                    }
                    queued_len = agent.follow_up_queue.len();
                }
            }
            format!("Queued follow-up message (#{}).", queued_len)
        }),
        "queue" => Some({
            if matches!(args.first().map(|s| s.to_ascii_lowercase()), Some(cmd) if cmd == "clear") {
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut app.state
                {
                    if let Some(agent) = agents.get_mut(*active_agent_index) {
                        agent.follow_up_queue.clear();
                        agent.steering_message = None;
                    }
                }
                return Some("Cleared queued steering and follow-up messages.".to_string());
            }
            if let AppState::Chat {
                agents,
                active_agent_index,
                ..
            } = &app.state
            {
                if let Some(agent) = agents.get(*active_agent_index) {
                    let steering = if agent.steering_message.is_some() {
                        "yes"
                    } else {
                        "no"
                    };
                    let next_followup = agent
                        .follow_up_queue
                        .front()
                        .map(|m| {
                            if m.chars().count() > 80 {
                                format!("{}...", m.chars().take(80).collect::<String>())
                            } else {
                                m.clone()
                            }
                        })
                        .unwrap_or_else(|| "(none)".to_string());
                    return Some(format!(
                        "Queue status:\n  steering: {}\n  follow-ups: {}\n  next: {}",
                        steering,
                        agent.follow_up_queue.len(),
                        next_followup
                    ));
                }
            }
            "Queue unavailable in current state.".to_string()
        }),
        "messages" => Some({
            let limit = args.first().and_then(|s| s.parse().ok()).unwrap_or(10);
            format!("Message history not implemented yet. (limit: {})", limit)
        }),
        "last_error" => Some(if let AppState::Chat { messages, .. } = &app.state {
            let maybe_error = messages.iter().rev().find_map(|m| {
                if m.role != MessageRole::System {
                    return None;
                }
                let text = m
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.to_lowercase().contains("failed") || text.to_lowercase().contains("error") {
                    Some(text)
                } else {
                    None
                }
            });
            maybe_error.unwrap_or_else(|| "No recent error found.".to_string())
        } else {
            "Not in a chat session.".to_string()
        }),
        "task" => Some(if let AppState::Chat { tasks, .. } = &mut app.state {
            match args.first().copied() {
                Some("add") => {
                    if args.len() < 2 {
                        return Some("Usage: /task add <description>".to_string());
                    }
                    let description = args[1..].join(" ");
                    let id = format!("task-{}", tasks.len() + 1);
                    tasks.push(crate::app::Task {
                        id: id.clone(),
                        description: description.clone(),
                        status: TaskStatus::Pending,
                        pinned: false,
                    });
                    format!("Task added: {} (ID: {})", description, id)
                }
                Some("done") | Some("fail") | Some("work") | Some("pending") => {
                    if args.len() < 2 {
                        return Some("Usage: /task <status> <id>".to_string());
                    }
                    let id = args[1];
                    if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                        match args[0] {
                            "done" => task.status = TaskStatus::Done,
                            "fail" => task.status = TaskStatus::Failed,
                            "work" => task.status = TaskStatus::Working,
                            "pending" => task.status = TaskStatus::Pending,
                            _ => {}
                        }
                        format!("Task {} marked as {}", id, args[0])
                    } else {
                        format!("Task not found: {}", id)
                    }
                }
                Some("pin") => {
                    if args.len() < 2 {
                        return Some("Usage: /task pin <id>".to_string());
                    }
                    let id = args[1];
                    if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                        task.pinned = !task.pinned;
                        format!("Task {} pinned: {}", id, task.pinned)
                    } else {
                        format!("Task not found: {}", id)
                    }
                }
                Some("list") => {
                    if tasks.is_empty() {
                        "No tasks.".to_string()
                    } else {
                        let lines: Vec<String> = tasks
                            .iter()
                            .map(|t| {
                                format!(
                                    "[{}] {} ({:?}) - Pinned: {}",
                                    t.id, t.description, t.status, t.pinned
                                )
                            })
                            .collect();
                        format!("Tasks:\n{}", lines.join("\n"))
                    }
                }
                _ => "Usage: /task add|done|fail|work|pin|list ...".to_string(),
            }
        } else {
            "Not in a chat session.".to_string()
        }),
        "prompt" => Some({
            let text = args.join(" ");
            if text.is_empty() {
                return Some("Usage: /prompt <text...>".to_string());
            }
            let (session_id, active_agent_id) = if let AppState::Chat {
                session_id,
                agents,
                active_agent_index,
                ..
            } = &mut app.state
            {
                let agent_id = agents
                    .get(*active_agent_index)
                    .map(|a| a.agent_id.clone())
                    .unwrap_or_else(|| "A1".to_string());
                (session_id.clone(), agent_id)
            } else {
                (String::new(), "A1".to_string())
            };

            if session_id.is_empty() {
                return Some("Not in a chat session. Use /use <session_id> first.".to_string());
            }
            app.dispatch_prompt_for_agent(session_id, active_agent_id, text);
            "Prompt sent.".to_string()
        }),
        "title" => Some({
            let new_title = args.join(" ");
            if new_title.is_empty() {
                return Some("Usage: /title <new title...>".to_string());
            }
            if let AppState::Chat { session_id, .. } = &mut app.state {
                if let Some(client) = &app.client {
                    let req = crate::net::client::UpdateSessionRequest {
                        title: Some(new_title.clone()),
                        ..Default::default()
                    };
                    if let Ok(_session) = client.update_session(session_id, req).await {
                        if let Some(s) = app.sessions.iter_mut().find(|s| &s.id == session_id) {
                            s.title = new_title.clone();
                        }
                        return Some(format!("Session renamed to: {}", new_title));
                    }
                }
                "Failed to rename session.".to_string()
            } else {
                "Not in a chat session.".to_string()
            }
        }),
        "missions" => Some({
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            match client.mission_list().await {
                Ok(missions) => {
                    if missions.is_empty() {
                        return Some("No missions found.".to_string());
                    }
                    let lines = missions
                        .into_iter()
                        .map(|mission| {
                            format!(
                                "- {} [{}] {} (work_items={})",
                                mission.mission_id,
                                format!("{:?}", mission.status).to_lowercase(),
                                mission.spec.title,
                                mission.work_items.len()
                            )
                        })
                        .collect::<Vec<_>>();
                    format!("Missions:\n{}", lines.join("\n"))
                }
                Err(err) => format!("Failed to list missions: {}", err),
            }
        }),
        "mission_create" => Some({
            if args.is_empty() {
                return Some(
                    "Usage: /mission_create <title> :: <goal> [:: work_item_title]".to_string(),
                );
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let raw = args.join(" ");
            let segments = raw
                .split("::")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            if segments.len() < 2 {
                return Some(
                    "Usage: /mission_create <title> :: <goal> [:: work_item_title]".to_string(),
                );
            }
            let work_items = if let Some(work_item_title) = segments.get(2) {
                vec![crate::net::client::MissionCreateWorkItem {
                    work_item_id: None,
                    title: (*work_item_title).to_string(),
                    detail: None,
                    assigned_agent: None,
                }]
            } else {
                vec![crate::net::client::MissionCreateWorkItem {
                    work_item_id: None,
                    title: "Initial implementation".to_string(),
                    detail: Some("Auto-seeded work item".to_string()),
                    assigned_agent: None,
                }]
            };
            let request = crate::net::client::MissionCreateRequest {
                title: segments[0].to_string(),
                goal: segments[1].to_string(),
                work_items,
            };
            match client.mission_create(request).await {
                Ok(mission) => format!(
                    "Created mission {}: {}",
                    mission.mission_id, mission.spec.title
                ),
                Err(err) => format!("Failed to create mission: {}", err),
            }
        }),
        "mission_get" => Some({
            if args.len() != 1 {
                return Some("Usage: /mission_get <mission_id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            match client.mission_get(args[0]).await {
                Ok(mission) => {
                    let item_lines = mission
                        .work_items
                        .iter()
                        .map(|item| {
                            format!(
                                "- {} [{}]",
                                item.title,
                                format!("{:?}", item.status).to_lowercase()
                            )
                        })
                        .collect::<Vec<_>>();
                    format!(
                        "Mission {} [{}]\nTitle: {}\nGoal: {}\nWork Items:\n{}",
                        mission.mission_id,
                        format!("{:?}", mission.status).to_lowercase(),
                        mission.spec.title,
                        mission.spec.goal,
                        if item_lines.is_empty() {
                            "- (none)".to_string()
                        } else {
                            item_lines.join("\n")
                        }
                    )
                }
                Err(err) => format!("Failed to get mission: {}", err),
            }
        }),
        "mission_event" => Some({
            if args.len() < 2 {
                return Some("Usage: /mission_event <mission_id> <event_json>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let mission_id = args[0];
            let raw_json = args[1..].join(" ");
            let event = match serde_json::from_str::<Value>(&raw_json) {
                Ok(value) => value,
                Err(err) => return Some(format!("Invalid event JSON: {}", err)),
            };
            match client.mission_apply_event(mission_id, event).await {
                Ok(result) => format!(
                    "Applied event to mission {} (revision={}, commands={})",
                    result.mission.mission_id,
                    result.mission.revision,
                    result.commands.len()
                ),
                Err(err) => format!("Failed to apply mission event: {}", err),
            }
        }),
        "mission_start" => Some({
            if args.len() != 1 {
                return Some("Usage: /mission_start <mission_id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let mission_id = args[0];
            let event = serde_json::json!({
                "type": "mission_started",
                "mission_id": mission_id
            });
            match client.mission_apply_event(mission_id, event).await {
                Ok(result) => format!(
                    "Mission started {} (revision={})",
                    result.mission.mission_id, result.mission.revision
                ),
                Err(err) => format!("Failed to start mission: {}", err),
            }
        }),
        "mission_review_ok" => Some({
            if args.len() < 2 {
                return Some(
                    "Usage: /mission_review_ok <mission_id> <work_item_id> [approval_id]"
                        .to_string(),
                );
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let mission_id = args[0];
            let work_item_id = args[1];
            let approval_id = args.get(2).copied().unwrap_or("review-1");
            let event = serde_json::json!({
                "type": "approval_granted",
                "mission_id": mission_id,
                "work_item_id": work_item_id,
                "approval_id": approval_id
            });
            match client.mission_apply_event(mission_id, event).await {
                Ok(result) => format!(
                    "Review approved for {}:{} (revision={})",
                    mission_id, work_item_id, result.mission.revision
                ),
                Err(err) => format!("Failed to approve review: {}", err),
            }
        }),
        "mission_test_ok" => Some({
            if args.len() < 2 {
                return Some(
                    "Usage: /mission_test_ok <mission_id> <work_item_id> [approval_id]".to_string(),
                );
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let mission_id = args[0];
            let work_item_id = args[1];
            let approval_id = args.get(2).copied().unwrap_or("test-1");
            let event = serde_json::json!({
                "type": "approval_granted",
                "mission_id": mission_id,
                "work_item_id": work_item_id,
                "approval_id": approval_id
            });
            match client.mission_apply_event(mission_id, event).await {
                Ok(result) => format!(
                    "Test approved for {}:{} (revision={})",
                    mission_id, work_item_id, result.mission.revision
                ),
                Err(err) => format!("Failed to approve test: {}", err),
            }
        }),
        "mission_review_no" => Some({
            if args.len() < 2 {
                return Some(
                    "Usage: /mission_review_no <mission_id> <work_item_id> [reason]".to_string(),
                );
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let mission_id = args[0];
            let work_item_id = args[1];
            let reason = if args.len() > 2 {
                args[2..].join(" ")
            } else {
                "needs_revision".to_string()
            };
            let event = serde_json::json!({
                "type": "approval_denied",
                "mission_id": mission_id,
                "work_item_id": work_item_id,
                "approval_id": "review-1",
                "reason": reason
            });
            match client.mission_apply_event(mission_id, event).await {
                Ok(result) => format!(
                    "Review denied for {}:{} (revision={})",
                    mission_id, work_item_id, result.mission.revision
                ),
                Err(err) => format!("Failed to deny review: {}", err),
            }
        }),
