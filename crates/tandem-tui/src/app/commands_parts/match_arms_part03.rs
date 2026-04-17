        "routine_resume" => Some({
            if args.len() != 1 {
                return Some("Usage: /routine_resume <id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let routine_id = args[0];
            let request = crate::net::client::RoutinePatchRequest {
                status: Some(crate::net::client::RoutineStatus::Active),
                ..Default::default()
            };
            match client.routines_patch(routine_id, request).await {
                Ok(_) => format!("Resumed routine {}.", routine_id),
                Err(err) => format!("Failed to resume routine: {}", err),
            }
        }),
        "routine_run_now" => Some({
            if args.is_empty() {
                return Some("Usage: /routine_run_now <id> [run_count]".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let routine_id = args[0];
            let run_count = if args.len() > 1 {
                match args[1].parse::<u32>() {
                    Ok(count) if count > 0 => Some(count),
                    _ => return Some("run_count must be a positive integer.".to_string()),
                }
            } else {
                None
            };
            let request = crate::net::client::RoutineRunNowRequest {
                run_count,
                reason: Some("manual_tui".to_string()),
            };
            match client.routines_run_now(routine_id, request).await {
                Ok(resp) => format!(
                    "Triggered routine {} (run_count={}).",
                    resp.routine_id, resp.run_count
                ),
                Err(err) => format!("Failed to trigger routine: {}", err),
            }
        }),
        "routine_delete" => Some({
            if args.len() != 1 {
                return Some("Usage: /routine_delete <id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let routine_id = args[0];
            match client.routines_delete(routine_id).await {
                Ok(true) => format!("Deleted routine {}.", routine_id),
                Ok(false) => format!("Routine not found: {}", routine_id),
                Err(err) => format!("Failed to delete routine: {}", err),
            }
        }),
        "routine_history" => Some({
            if args.is_empty() {
                return Some("Usage: /routine_history <id> [limit]".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let routine_id = args[0];
            let limit = if args.len() > 1 {
                match args[1].parse::<usize>() {
                    Ok(value) => Some(value),
                    Err(_) => return Some("limit must be a positive integer.".to_string()),
                }
            } else {
                Some(10)
            };
            match client.routines_history(routine_id, limit).await {
                Ok(events) => {
                    if events.is_empty() {
                        return Some(format!("No history for routine {}.", routine_id));
                    }
                    let lines = events
                        .iter()
                        .map(|event| {
                            format!(
                                "- {} run_count={} status={} at={}",
                                event.trigger_type,
                                event.run_count,
                                event.status,
                                event.fired_at_ms
                            )
                        })
                        .collect::<Vec<_>>();
                    format!("Routine history ({}):\n{}", routine_id, lines.join("\n"))
                }
                Err(err) => format!("Failed to load routine history: {}", err),
            }
        }),
        "config" => Some({
            let lines = vec![
                format!(
                    "Engine URL: {}",
                    app.client
                        .as_ref()
                        .map(|c| c.base_url())
                        .unwrap_or(&"not connected")
                ),
                format!("Sessions: {}", app.sessions.len()),
                format!("Current Mode: {:?}", app.current_mode),
                format!(
                    "Current Provider: {}",
                    app.current_provider.as_deref().unwrap_or("none")
                ),
                format!(
                    "Current Model: {}",
                    app.current_model.as_deref().unwrap_or("none")
                ),
            ];
            format!("Configuration:\n{}", lines.join("\n"))
        }),
        "requests" => Some({
            if let AppState::Chat {
                pending_requests,
                modal,
                request_cursor,
                ..
            } = &mut app.state
            {
                if pending_requests.is_empty() {
                    "No pending requests.".to_string()
                } else {
                    if *request_cursor >= pending_requests.len() {
                        *request_cursor = pending_requests.len().saturating_sub(1);
                    }
                    *modal = Some(ModalState::RequestCenter);
                    format!(
                        "Opened request center ({} pending).",
                        pending_requests.len()
                    )
                }
            } else {
                "Requests are only available in chat mode.".to_string()
            }
        }),
        "copy" => Some({
            if let AppState::Chat { messages, .. } = &app.state {
                match app.copy_latest_assistant_to_clipboard(messages) {
                    Ok(len) => format!("Copied {} characters to clipboard.", len),
                    Err(err) => format!("Clipboard copy failed: {}", err),
                }
            } else {
                "Clipboard copy works in chat screens only.".to_string()
            }
        }),
        "approve" | "deny" | "answer" => Some({
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let session_id = if let AppState::Chat { session_id, .. } = &app.state {
                Some(session_id.clone())
            } else {
                None
            };

            match cmd_name {
                "approve" => {
                    if args
                        .first()
                        .map(|s| s.eq_ignore_ascii_case("all"))
                        .unwrap_or(false)
                        || args.is_empty()
                    {
                        let Ok(snapshot) = client.list_permissions().await else {
                            return Some("Failed to load pending permissions.".to_string());
                        };
                        let pending: Vec<String> = snapshot
                            .requests
                            .iter()
                            .filter(|r| r.status.as_deref() == Some("pending"))
                            .filter(|r| {
                                if let Some(sid) = &session_id {
                                    r.session_id.as_deref() == Some(sid.as_str())
                                } else {
                                    true
                                }
                            })
                            .map(|r| r.id.clone())
                            .collect();
                        if pending.is_empty() {
                            return Some("No pending permissions.".to_string());
                        }
                        let mut approved = 0usize;
                        for id in pending {
                            if client.reply_permission(&id, "allow").await.unwrap_or(false) {
                                approved += 1;
                            }
                        }
                        format!("Approved {} pending permission request(s).", approved)
                    } else {
                        let id = args[0];
                        let reply = if args
                            .get(1)
                            .map(|s| s.eq_ignore_ascii_case("always"))
                            .unwrap_or(false)
                        {
                            "always"
                        } else {
                            "allow"
                        };
                        if client.reply_permission(id, reply).await.unwrap_or(false) {
                            format!("Approved permission request {}.", id)
                        } else {
                            format!("Permission request not found: {}", id)
                        }
                    }
                }
                "deny" => {
                    if args.is_empty() {
                        return Some("Usage: /deny <id>".to_string());
                    }
                    let id = args[0];
                    if client.reply_permission(id, "deny").await.unwrap_or(false) {
                        format!("Denied permission request {}.", id)
                    } else {
                        format!("Permission request not found: {}", id)
                    }
                }
                "answer" => {
                    if args.is_empty() {
                        return Some("Usage: /answer <id> <text>".to_string());
                    }
                    let id = args[0];
                    let reply = if args.len() > 1 {
                        args[1..].join(" ")
                    } else {
                        "allow".to_string()
                    };
                    if client
                        .reply_permission(id, reply.as_str())
                        .await
                        .unwrap_or(false)
                    {
                        format!("Replied to permission request {}.", id)
                    } else {
                        format!("Permission request not found: {}", id)
                    }
                }
                _ => "Unsupported permission command.".to_string(),
            }
        }),
        "mode" => Some(if args.is_empty() {
            let agent = app.current_mode.as_agent();
            format!("Current mode: {:?} (agent: {})", app.current_mode, agent)
        } else {
            let mode_name = args[0];
            if let Some(mode) = TandemMode::from_str(mode_name) {
                app.current_mode = mode;
                format!("Mode set to: {:?}", mode)
            } else {
                format!(
                    "Unknown mode: {}. Use /modes to see available modes.",
                    mode_name
                )
            }
        }),
        "modes" => Some({
            let lines: Vec<String> = TandemMode::all_modes()
                .iter()
                .map(|(name, desc)| format!("  {} - {}", name, desc))
                .collect();
            format!("Available modes:\n{}", lines.join("\n"))
        }),
        "providers" => Some(if let Some(catalog) = &app.provider_catalog {
            let lines: Vec<String> = catalog
                .all
                .iter()
                .map(|p| {
                    let status = if catalog.connected.contains(&p.id) {
                        "connected"
                    } else {
                        "not configured"
                    };
                    format!("  {} - {}", p.id, status)
                })
                .collect();
            if lines.is_empty() {
                "No providers available.".to_string()
            } else {
                format!("Available providers:\n{}", lines.join("\n"))
            }
        } else {
            "Loading providers... (use /providers to refresh)".to_string()
        }),
        "provider" => Some({
            let mut step = SetupStep::SelectProvider;
            let mut selected_provider_index = 0;
            let filter_model = String::new();

            if !args.is_empty() {
                let provider_id = args[0];
                if let Some(catalog) = &app.provider_catalog {
                    if let Some(idx) = catalog.all.iter().position(|p| p.id == provider_id) {
                        selected_provider_index = idx;
                        step = if catalog.connected.contains(&provider_id.to_string()) {
                            SetupStep::SelectModel
                        } else {
                            SetupStep::EnterApiKey
                        };
                    }
                }
            } else if let Some(current) = &app.current_provider {
                if let Some(catalog) = &app.provider_catalog {
                    if let Some(idx) = catalog.all.iter().position(|p| &p.id == current) {
                        selected_provider_index = idx;
                        step = if catalog.connected.contains(current) {
                            SetupStep::SelectModel
                        } else {
                            SetupStep::EnterApiKey
                        };
                    }
                }
            }

            app.state = AppState::SetupWizard {
                step,
                provider_catalog: app.provider_catalog.clone(),
                selected_provider_index,
                selected_model_index: 0,
                api_key_input: String::new(),
                model_input: filter_model,
            };
            "Opening provider selection...".to_string()
        }),
        "models" => Some({
            let provider_id = args
                .first()
                .map(|s| s.to_string())
                .or_else(|| app.current_provider.clone());
            if let Some(catalog) = &app.provider_catalog {
                if let Some(pid) = &provider_id {
                    if let Some(provider) = catalog.all.iter().find(|p| p.id == *pid) {
                        let model_ids: Vec<String> = provider.models.keys().cloned().collect();
                        if model_ids.is_empty() {
                            format!("No models available for provider: {}", pid)
                        } else {
                            format!(
                                "Models for {}:\n{}",
                                pid,
                                model_ids
                                    .iter()
                                    .map(|m| format!("  {}", m))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            )
                        }
                    } else {
                        format!("Provider not found: {}", pid)
                    }
                } else {
                    "No provider selected. Use /provider <id> first.".to_string()
                }
            } else {
                "Loading providers... (use /providers to refresh)".to_string()
            }
        }),
        "model" => Some(if args.is_empty() {
            let mut selected_provider_index = 0;
            if let Some(current) = &app.current_provider {
                if let Some(catalog) = &app.provider_catalog {
                    if let Some(idx) = catalog.all.iter().position(|p| &p.id == current) {
                        selected_provider_index = idx;
                    }
                }
            }
            app.state = AppState::SetupWizard {
                step: SetupStep::SelectModel,
                provider_catalog: app.provider_catalog.clone(),
                selected_provider_index,
                selected_model_index: 0,
                api_key_input: String::new(),
                model_input: String::new(),
            };
            "Opening model selection...".to_string()
        } else {
            let model_id = args.join(" ");
            app.current_model = Some(model_id.clone());
            app.pending_model_provider = None;
            if let Some(provider_id) = app.current_provider.clone() {
                app.persist_provider_defaults(&provider_id, Some(&model_id), None)
                    .await;
            }
            format!("Model set to: {}", model_id)
        }),
