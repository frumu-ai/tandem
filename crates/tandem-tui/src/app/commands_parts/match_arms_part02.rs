        "agent-team" | "agent_team" => Some({
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let sub = args.first().copied().unwrap_or("summary");
            match sub {
                "summary" => {
                    let missions = client.agent_team_missions().await;
                    let instances = client.agent_team_instances(None).await;
                    let approvals = client.agent_team_approvals().await;
                    match (missions, instances, approvals) {
                        (Ok(missions), Ok(instances), Ok(approvals)) => format!(
                            "Agent-Team Summary:\n  Missions: {}\n  Instances: {}\n  Spawn approvals: {}\n  Tool approvals: {}",
                            missions.len(),
                            instances.len(),
                            approvals.spawn_approvals.len(),
                            approvals.tool_approvals.len()
                        ),
                        _ => "Failed to load agent-team summary.".to_string(),
                    }
                }
                "missions" => match client.agent_team_missions().await {
                    Ok(missions) => {
                        if missions.is_empty() {
                            return Some("No agent-team missions found.".to_string());
                        }
                        let lines = missions
                            .into_iter()
                            .map(|mission| {
                                format!(
                                    "- {} total={} running={} done={} failed={} cancelled={}",
                                    mission.mission_id,
                                    mission.instance_count,
                                    mission.running_count,
                                    mission.completed_count,
                                    mission.failed_count,
                                    mission.cancelled_count
                                )
                            })
                            .collect::<Vec<_>>();
                        format!("Agent-Team Missions:\n{}", lines.join("\n"))
                    }
                    Err(err) => format!("Failed to list agent-team missions: {}", err),
                },
                "instances" => {
                    let mission_id = args.get(1).copied();
                    match client.agent_team_instances(mission_id).await {
                        Ok(instances) => {
                            if instances.is_empty() {
                                return Some("No agent-team instances found.".to_string());
                            }
                            let lines = instances
                                .into_iter()
                                .map(|instance| {
                                    format!(
                                        "- {} role={} mission={} status={} parent={}",
                                        instance.instance_id,
                                        instance.role,
                                        instance.mission_id,
                                        instance.status,
                                        instance
                                            .parent_instance_id
                                            .unwrap_or_else(|| "-".to_string())
                                    )
                                })
                                .collect::<Vec<_>>();
                            format!("Agent-Team Instances:\n{}", lines.join("\n"))
                        }
                        Err(err) => format!("Failed to list agent-team instances: {}", err),
                    }
                }
                "approvals" => match client.agent_team_approvals().await {
                    Ok(approvals) => {
                        let mut lines = Vec::new();
                        for spawn in approvals.spawn_approvals {
                            lines.push(format!("- spawn approval {}", spawn.approval_id));
                        }
                        for tool in approvals.tool_approvals {
                            lines.push(format!(
                                "- tool approval {} ({})",
                                tool.approval_id,
                                tool.tool.unwrap_or_else(|| "tool".to_string())
                            ));
                        }
                        if lines.is_empty() {
                            "No agent-team approvals pending.".to_string()
                        } else {
                            format!("Agent-Team Approvals:\n{}", lines.join("\n"))
                        }
                    }
                    Err(err) => format!("Failed to list agent-team approvals: {}", err),
                },
                "bindings" => {
                    let team_filter = args.get(1).copied();
                    App::format_local_agent_team_bindings(team_filter)
                }
                "approve" => {
                    if args.len() < 3 {
                        return Some(
                            "Usage: /agent-team approve <spawn|tool> <id> [reason]".to_string(),
                        );
                    }
                    let target = args[1];
                    let id = args[2];
                    let reason = if args.len() > 3 {
                        args[3..].join(" ")
                    } else {
                        "approved in TUI".to_string()
                    };
                    match target {
                        "spawn" => match client.agent_team_approve_spawn(id, &reason).await {
                            Ok(true) => format!("Approved spawn approval {}.", id),
                            Ok(false) => format!("Spawn approval not found or denied: {}", id),
                            Err(err) => format!("Failed to approve spawn approval: {}", err),
                        },
                        "tool" => match client.reply_permission(id, "allow").await {
                            Ok(true) => format!("Approved tool request {}.", id),
                            Ok(false) => format!("Tool request not found: {}", id),
                            Err(err) => format!("Failed to approve tool request: {}", err),
                        },
                        _ => "Usage: /agent-team approve <spawn|tool> <id> [reason]".to_string(),
                    }
                }
                "deny" => {
                    if args.len() < 3 {
                        return Some(
                            "Usage: /agent-team deny <spawn|tool> <id> [reason]".to_string(),
                        );
                    }
                    let target = args[1];
                    let id = args[2];
                    let reason = if args.len() > 3 {
                        args[3..].join(" ")
                    } else {
                        "denied in TUI".to_string()
                    };
                    match target {
                        "spawn" => match client.agent_team_deny_spawn(id, &reason).await {
                            Ok(true) => format!("Denied spawn approval {}.", id),
                            Ok(false) => {
                                format!("Spawn approval not found or already resolved: {}", id)
                            }
                            Err(err) => format!("Failed to deny spawn approval: {}", err),
                        },
                        "tool" => match client.reply_permission(id, "deny").await {
                            Ok(true) => format!("Denied tool request {}.", id),
                            Ok(false) => format!("Tool request not found: {}", id),
                            Err(err) => format!("Failed to deny tool request: {}", err),
                        },
                        _ => "Usage: /agent-team deny <spawn|tool> <id> [reason]".to_string(),
                    }
                }
                _ => {
                    "Usage: /agent-team [summary|missions|instances [mission_id]|approvals|bindings [team]|approve <spawn|tool> <id> [reason]|deny <spawn|tool> <id> [reason]]".to_string()
                }
            }
        }),
        "preset" | "presets" => Some({
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let sub = args.first().copied().unwrap_or("help").to_ascii_lowercase();
            match sub.as_str() {
                "index" => match client.presets_index().await {
                    Ok(index) => format!(
                        "Preset index:\n  skill_modules: {}\n  agent_presets: {}\n  automation_presets: {}\n  pack_presets: {}\n  generated_at_ms: {}",
                        index.skill_modules.len(),
                        index.agent_presets.len(),
                        index.automation_presets.len(),
                        index.pack_presets.len(),
                        index.generated_at_ms
                    ),
                    Err(err) => format!("Failed to load preset index: {}", err),
                },
                "agent" => {
                    let action = args.get(1).copied().unwrap_or("help").to_ascii_lowercase();
                    match action.as_str() {
                        "compose" => {
                            let tail = args.get(2..).unwrap_or(&[]).join(" ");
                            let mut pieces = tail.splitn(2, "::");
                            let base_prompt = pieces.next().unwrap_or("").trim();
                            let fragments_raw = pieces.next().unwrap_or("").trim();
                            if base_prompt.is_empty() || fragments_raw.is_empty() {
                                return Some(
                                    "Usage: /preset agent compose <base_prompt> :: <fragments_json>"
                                        .to_string(),
                                );
                            }
                            let fragments_json =
                                match serde_json::from_str::<Value>(fragments_raw) {
                                    Ok(value) if value.is_array() => value,
                                    Ok(_) => {
                                        return Some(
                                            "fragments_json must be a JSON array of {id,phase,content}"
                                                .to_string(),
                                        );
                                    }
                                    Err(err) => return Some(format!("Invalid fragments_json: {}", err)),
                                };
                            let request = json!({
                                "base_prompt": base_prompt,
                                "fragments": fragments_json,
                            });
                            match client.presets_compose_preview(request).await {
                                Ok(payload) => {
                                    let composition =
                                        payload.get("composition").cloned().unwrap_or(payload);
                                    format!(
                                        "Agent compose preview:\n{}",
                                        serde_json::to_string_pretty(&composition)
                                            .unwrap_or_else(|_| "{}".to_string())
                                    )
                                }
                                Err(err) => format!("Compose preview failed: {}", err),
                            }
                        }
                        "summary" => {
                            let tail = args.get(2..).unwrap_or(&[]).join(" ");
                            let (required, optional) =
                                App::parse_required_optional_segments(&tail);
                            let request = json!({
                                "agent": {
                                    "required": required,
                                    "optional": optional,
                                },
                                "tasks": [],
                            });
                            match client.presets_capability_summary(request).await {
                                Ok(payload) => {
                                    let summary = payload.get("summary").cloned().unwrap_or(payload);
                                    format!(
                                        "Agent capability summary:\n{}",
                                        serde_json::to_string_pretty(&summary)
                                            .unwrap_or_else(|_| "{}".to_string())
                                    )
                                }
                                Err(err) => format!("Capability summary failed: {}", err),
                            }
                        }
                        "fork" => {
                            if args.len() < 3 {
                                return Some(
                                    "Usage: /preset agent fork <source_path> [target_id]".to_string(),
                                );
                            }
                            let source_path = args[2];
                            let target_id = args.get(3).copied();
                            let request = json!({
                                "kind": "agent_preset",
                                "source_path": source_path,
                                "target_id": target_id,
                            });
                            match client.presets_fork(request).await {
                                Ok(payload) => format!(
                                    "Forked agent preset override:\n{}",
                                    serde_json::to_string_pretty(&payload)
                                        .unwrap_or_else(|_| "{}".to_string())
                                ),
                                Err(err) => format!("Agent preset fork failed: {}", err),
                            }
                        }
                        _ => "Usage: /preset agent <compose|summary|fork> ...".to_string(),
                    }
                }
                "automation" => {
                    let action = args.get(1).copied().unwrap_or("help").to_ascii_lowercase();
                    match action.as_str() {
                        "summary" => {
                            let tail = args.get(2..).unwrap_or(&[]).join(" ");
                            let segments = tail
                                .split("::")
                                .map(str::trim)
                                .filter(|part| !part.is_empty())
                                .collect::<Vec<_>>();
                            if segments.is_empty() {
                                return Some("Usage: /preset automation summary <tasks_json> [:: required=<csv> :: optional=<csv>]".to_string());
                            }
                            let tasks_json = match serde_json::from_str::<Value>(segments[0]) {
                                Ok(value) => value,
                                Err(err) => return Some(format!("Invalid tasks_json: {}", err)),
                            };
                            let tasks = match App::normalize_automation_tasks(&tasks_json) {
                                Ok(items) => items,
                                Err(err) => return Some(err),
                            };
                            let (required, optional) = if segments.len() > 1 {
                                App::parse_required_optional_segments(&segments[1..].join(" :: "))
                            } else {
                                (Vec::new(), Vec::new())
                            };
                            let capability_tasks = tasks
                                .iter()
                                .map(|task| {
                                    json!({
                                        "required": task.get("required").cloned().unwrap_or_else(|| json!([])),
                                        "optional": task.get("optional").cloned().unwrap_or_else(|| json!([])),
                                    })
                                })
                                .collect::<Vec<_>>();
                            let request = json!({
                                "agent": {
                                    "required": required,
                                    "optional": optional,
                                },
                                "tasks": capability_tasks,
                            });
                            match client.presets_capability_summary(request).await {
                                Ok(payload) => {
                                    let summary = payload.get("summary").cloned().unwrap_or(payload);
                                    format!(
                                        "Automation capability summary ({} tasks):\n{}",
                                        tasks.len(),
                                        serde_json::to_string_pretty(&summary)
                                            .unwrap_or_else(|_| "{}".to_string())
                                    )
                                }
                                Err(err) => format!("Automation summary failed: {}", err),
                            }
                        }
                        "save" => {
                            let tail = args.get(2..).unwrap_or(&[]).join(" ");
                            let segments = tail
                                .split("::")
                                .map(str::trim)
                                .filter(|part| !part.is_empty())
                                .collect::<Vec<_>>();
                            if segments.len() < 2 {
                                return Some("Usage: /preset automation save <id> :: <tasks_json> [:: required=<csv> :: optional=<csv>]".to_string());
                            }
                            let id = segments[0];
                            if id.is_empty() {
                                return Some("Automation preset id is required.".to_string());
                            }
                            let tasks_json = match serde_json::from_str::<Value>(segments[1]) {
                                Ok(value) => value,
                                Err(err) => return Some(format!("Invalid tasks_json: {}", err)),
                            };
                            let tasks = match App::normalize_automation_tasks(&tasks_json) {
                                Ok(items) => items,
                                Err(err) => return Some(err),
                            };
                            let (required, optional) = if segments.len() > 2 {
                                App::parse_required_optional_segments(&segments[2..].join(" :: "))
                            } else {
                                (Vec::new(), Vec::new())
                            };
                            let capability_tasks = tasks
                                .iter()
                                .map(|task| {
                                    json!({
                                        "required": task.get("required").cloned().unwrap_or_else(|| json!([])),
                                        "optional": task.get("optional").cloned().unwrap_or_else(|| json!([])),
                                    })
                                })
                                .collect::<Vec<_>>();
                            let summary_request = json!({
                                "agent": {
                                    "required": required,
                                    "optional": optional,
                                },
                                "tasks": capability_tasks,
                            });
                            let summary_payload =
                                match client.presets_capability_summary(summary_request).await {
                                    Ok(payload) => payload,
                                    Err(err) => {
                                        return Some(format!("Automation summary failed: {}", err));
                                    }
                                };
                            let summary = summary_payload
                                .get("summary")
                                .cloned()
                                .unwrap_or_else(|| json!({}));
                            let yaml = App::automation_override_yaml(id, &tasks, &summary);
                            match client
                                .presets_override_put("automation_preset", id, &yaml)
                                .await
                            {
                                Ok(payload) => format!(
                                    "Saved automation preset override `{}` with {} task(s).\n{}",
                                    id,
                                    tasks.len(),
                                    serde_json::to_string_pretty(&payload)
                                        .unwrap_or_else(|_| "{}".to_string())
                                ),
                                Err(err) => format!("Automation override save failed: {}", err),
                            }
                        }
                        _ => "Usage: /preset automation <summary|save> ...".to_string(),
                    }
                }
                _ => "Usage: /preset <index|agent|automation> ...".to_string(),
            }
        }),
        "context_runs" => Some({
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let limit = args
                .first()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(20);
            match client.context_runs_list().await {
                Ok(mut runs) => {
                    if runs.is_empty() {
                        return Some("No context runs found.".to_string());
                    }
                    runs.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
                    let lines = runs
                        .into_iter()
                        .take(limit)
                        .map(|run| {
                            format!(
                                "- {} [{}] type={} steps={} updated_at={}\n  objective: {}",
                                run.run_id,
                                format!("{:?}", run.status).to_lowercase(),
                                run.run_type,
                                run.steps.len(),
                                run.updated_at_ms,
                                run.objective
                            )
                        })
                        .collect::<Vec<_>>();
                    format!("Context runs:\n{}", lines.join("\n"))
                }
                Err(err) => format!("Failed to list context runs: {}", err),
            }
        }),
        "context_run_create" => Some({
            if args.is_empty() {
                return Some("Usage: /context_run_create <objective...>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let objective = args.join(" ");
            match client
                .context_run_create(None, objective, Some("interactive".to_string()), None)
                .await
            {
                Ok(run) => format!("Created context run {} [{}].", run.run_id, run.run_type),
                Err(err) => format!("Failed to create context run: {}", err),
            }
        }),
        "context_run_get" => Some({
            if args.len() != 1 {
                return Some("Usage: /context_run_get <run_id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            match client.context_run_get(run_id).await {
                Ok(detail) => {
                    let run = detail.run;
                    let rollback_preview_steps = detail
                        .rollback_preview_summary
                        .get("step_count")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(0);
                    let rollback_history_entries = detail
                        .rollback_history_summary
                        .get("entry_count")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(0);
                    let rollback_policy_eligible = detail
                        .rollback_policy
                        .get("eligible")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);
                    let rollback_required_ack = detail
                        .rollback_policy
                        .get("required_policy_ack")
                        .and_then(|value| value.as_str())
                        .unwrap_or("<none>");
                    let last_rollback_outcome = detail
                        .last_rollback_outcome
                        .get("outcome")
                        .and_then(|value| value.as_str())
                        .unwrap_or("<none>");
                    let last_rollback_reason = detail
                        .last_rollback_outcome
                        .get("reason")
                        .and_then(|value| value.as_str())
                        .unwrap_or("<none>");
                    format!(
                        "Context run {}\n  status: {}\n  type: {}\n  revision: {}\n  workspace: {}\n  steps: {}\n  why_next_step: {}\n  objective: {}\n\nRollback\n  preview_steps: {}\n  history_entries: {}\n  policy: {}\n  required_ack: {}\n  last_outcome: {}\n  last_reason: {}\n\nNext\n  /context_run_rollback_preview {}\n  /context_run_rollback_history {}",
                        run.run_id,
                        format!("{:?}", run.status).to_lowercase(),
                        run.run_type,
                        run.revision,
                        run.workspace.canonical_path,
                        run.steps.len(),
                        run.why_next_step.unwrap_or_else(|| "<none>".to_string()),
                        run.objective,
                        rollback_preview_steps,
                        rollback_history_entries,
                        if rollback_policy_eligible {
                            "eligible"
                        } else {
                            "blocked"
                        },
                        rollback_required_ack,
                        last_rollback_outcome,
                        last_rollback_reason,
                        run.run_id,
                        run.run_id
                    )
                }
                Err(err) => format!("Failed to load context run: {}", err),
            }
        }),
        "context_run_rollback_preview" => Some({
            if args.len() != 1 {
                return Some("Usage: /context_run_rollback_preview <run_id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            match client.context_run_rollback_preview(run_id).await {
                Ok(preview) => {
                    if preview.steps.is_empty() {
                        return Some(format!(
                            "No rollback preview steps for context run {}.",
                            run_id
                        ));
                    }
                    let lines = preview
                        .steps
                        .iter()
                        .take(12)
                        .map(|step| {
                            format!(
                                "  - [{}] seq={} ops={} tool={} event={}",
                                if step.executable { "exec" } else { "info" },
                                step.seq,
                                step.operation_count,
                                step.tool.as_deref().unwrap_or("<unknown>"),
                                step.event_id
                            )
                        })
                        .collect::<Vec<_>>();
                    let executable_ids = preview
                        .steps
                        .iter()
                        .filter(|step| step.executable)
                        .map(|step| step.event_id.clone())
                        .collect::<Vec<_>>();
                    let executable_id_lines = if executable_ids.is_empty() {
                        "  <none>".to_string()
                    } else {
                        executable_ids
                            .iter()
                            .map(|event_id| format!("  {}", event_id))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    let next = if executable_ids.is_empty() {
                        "  No executable rollback steps are available yet.".to_string()
                    } else {
                        format!(
                            "  /context_run_rollback_execute {} --ack {}\n  /context_run_rollback_execute_all {} --ack",
                            run_id,
                            executable_ids.join(" "),
                            run_id
                        )
                    };
                    format!(
                        "Rollback preview ({})\n  step_count: {}\n  executable_steps: {}\n  advisory_steps: {}\n  fully_executable: {}\n\nExecutable ids\n{}\n\nSteps\n{}\n\nNext\n{}",
                        run_id,
                        preview.step_count,
                        preview.executable_step_count,
                        preview.advisory_step_count,
                        preview.executable,
                        executable_id_lines,
                        lines.join("\n"),
                        next
                    )
                }
                Err(err) => format!("Failed to load rollback preview: {}", err),
            }
        }),
        "context_run_rollback_execute" => Some({
            if args.len() < 3 || args[1] != "--ack" {
                return Some(
                    "Usage: /context_run_rollback_execute <run_id> --ack <event_id...>".to_string(),
                );
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            let event_ids = args[2..]
                .iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            if event_ids.is_empty() {
                return Some("Provide at least one rollback preview event id.".to_string());
            }
            match client
                .context_run_rollback_execute(
                    run_id,
                    event_ids.clone(),
                    Some("allow_rollback_execution".to_string()),
                )
                .await
            {
                Ok(result) => {
                    let missing = result
                        .missing_event_ids
                        .as_ref()
                        .filter(|rows| !rows.is_empty())
                        .map(|rows| rows.join(", "))
                        .unwrap_or_else(|| "<none>".to_string());
                    format!(
                        "Rollback execute ({})\n  applied: {}\n  selected: {}\n  applied_steps: {}\n  applied_operations: {}\n  missing: {}\n  reason: {}\n\nNext\n  /context_run_rollback_history {}\n  /context_run_rollback_preview {}",
                        run_id,
                        result.applied,
                        if result.selected_event_ids.is_empty() {
                            event_ids.join(", ")
                        } else {
                            result.selected_event_ids.join(", ")
                        },
                        result.applied_step_count.unwrap_or(0),
                        result.applied_operation_count.unwrap_or(0),
                        missing,
                        result.reason.unwrap_or_else(|| "<none>".to_string()),
                        run_id,
                        run_id
                    )
                }
                Err(err) => format!("Failed to execute rollback: {}", err),
            }
        }),
        "context_run_rollback_execute_all" => Some({
            if args.len() != 2 || args[1] != "--ack" {
                return Some("Usage: /context_run_rollback_execute_all <run_id> --ack".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            let preview = match client.context_run_rollback_preview(run_id).await {
                Ok(preview) => preview,
                Err(err) => return Some(format!("Failed to load rollback preview: {}", err)),
            };
            let event_ids = preview
                .steps
                .iter()
                .filter(|step| step.executable)
                .map(|step| step.event_id.clone())
                .collect::<Vec<_>>();
            if event_ids.is_empty() {
                return Some(format!(
                    "No executable rollback preview steps for context run {}.",
                    run_id
                ));
            }
            match client
                .context_run_rollback_execute(
                    run_id,
                    event_ids.clone(),
                    Some("allow_rollback_execution".to_string()),
                )
                .await
            {
                Ok(result) => {
                    let missing = result
                        .missing_event_ids
                        .as_ref()
                        .filter(|rows| !rows.is_empty())
                        .map(|rows| rows.join(", "))
                        .unwrap_or_else(|| "<none>".to_string());
                    let selected = if result.selected_event_ids.is_empty() {
                        event_ids.join(", ")
                    } else {
                        result.selected_event_ids.join(", ")
                    };
                    format!(
                        "Rollback execute all ({})\n  applied: {}\n  selected: {}\n  applied_steps: {}\n  applied_operations: {}\n  missing: {}\n  reason: {}\n\nNext\n  /context_run_rollback_history {}\n  /context_run_rollback_preview {}",
                        run_id,
                        result.applied,
                        selected,
                        result.applied_step_count.unwrap_or(0),
                        result.applied_operation_count.unwrap_or(0),
                        missing,
                        result.reason.unwrap_or_else(|| "<none>".to_string()),
                        run_id,
                        run_id
                    )
                }
                Err(err) => format!("Failed to execute rollback: {}", err),
            }
        }),
        "context_run_rollback_history" => Some({
            if args.len() != 1 {
                return Some("Usage: /context_run_rollback_history <run_id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            match client.context_run_rollback_history(run_id).await {
                Ok(history) => {
                    if history.entries.is_empty() {
                        return Some(format!("No rollback receipts for context run {}.", run_id));
                    }
                    let entry_count = history.entries.len();
                    let applied_count = history
                        .entries
                        .iter()
                        .filter(|entry| entry.outcome == "applied")
                        .count();
                    let blocked_count = history
                        .entries
                        .iter()
                        .filter(|entry| entry.outcome != "applied")
                        .count();
                    let lines = history
                        .entries
                        .iter()
                        .rev()
                        .take(6)
                        .map(|entry| {
                            let selected = if entry.selected_event_ids.is_empty() {
                                "<none>".to_string()
                            } else {
                                entry.selected_event_ids.join(", ")
                            };
                            let missing = entry
                                .missing_event_ids
                                .as_ref()
                                .filter(|rows| !rows.is_empty())
                                .map(|rows| rows.join(", "))
                                .unwrap_or_else(|| "<none>".to_string());
                            let actions = entry
                                .applied_by_action
                                .as_ref()
                                .filter(|counts| !counts.is_empty())
                                .map(|counts| {
                                    let mut rows = counts
                                        .iter()
                                        .map(|(action, count)| format!("{}={}", action, count))
                                        .collect::<Vec<_>>();
                                    rows.sort();
                                    rows.join(", ")
                                })
                                .unwrap_or_else(|| "<none>".to_string());
                            format!(
                                "  - seq={} outcome={} ts={}\n    selected: {}\n    missing: {}\n    steps: {}\n    operations: {}\n    actions: {}\n    reason: {}",
                                entry.seq,
                                entry.outcome,
                                entry.ts_ms,
                                selected,
                                missing,
                                entry.applied_step_count.unwrap_or(0),
                                entry.applied_operation_count.unwrap_or(0),
                                actions,
                                entry.reason.as_deref().unwrap_or("<none>")
                            )
                        })
                        .collect::<Vec<_>>();
                    format!(
                        "Rollback receipts ({})\n  entries: {}\n  applied: {}\n  blocked: {}\n\nRecent receipts\n{}",
                        run_id,
                        entry_count,
                        applied_count,
                        blocked_count,
                        lines.join("\n")
                    )
                }
                Err(err) => format!("Failed to load rollback receipts: {}", err),
            }
        }),
        "context_run_events" => Some({
            if args.is_empty() {
                return Some("Usage: /context_run_events <run_id> [tail]".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            let tail = if args.len() > 1 {
                match args[1].parse::<usize>() {
                    Ok(value) if value > 0 => Some(value),
                    _ => return Some("tail must be a positive integer.".to_string()),
                }
            } else {
                Some(20)
            };
            match client.context_run_events(run_id, None, tail).await {
                Ok(events) => {
                    if events.is_empty() {
                        return Some(format!("No events for context run {}.", run_id));
                    }
                    let lines = events
                        .iter()
                        .map(|event| {
                            format!(
                                "- #{} {} status={} step={} ts={}",
                                event.seq,
                                event.event_type,
                                format!("{:?}", event.status).to_lowercase(),
                                event.step_id.as_deref().unwrap_or("-"),
                                event.ts_ms
                            )
                        })
                        .collect::<Vec<_>>();
                    format!("Context run events ({}):\n{}", run_id, lines.join("\n"))
                }
                Err(err) => format!("Failed to load context run events: {}", err),
            }
        }),
        "context_run_pause" | "context_run_resume" | "context_run_cancel" => Some({
            if args.len() != 1 {
                return Some(format!("Usage: /{} <run_id>", cmd_name));
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            let (event_type, status, label) = match cmd_name {
                "context_run_pause" => (
                    "run_paused",
                    crate::net::client::ContextRunStatus::Paused,
                    "paused",
                ),
                "context_run_resume" => (
                    "run_resumed",
                    crate::net::client::ContextRunStatus::Running,
                    "running",
                ),
                _ => (
                    "run_cancelled",
                    crate::net::client::ContextRunStatus::Cancelled,
                    "cancelled",
                ),
            };
            match client
                .context_run_append_event(
                    run_id,
                    event_type,
                    status,
                    None,
                    json!({ "source": "tui" }),
                )
                .await
            {
                Ok(event) => format!(
                    "Context run {} {} (seq={} event={}).",
                    run_id, label, event.seq, event.event_id
                ),
                Err(err) => format!("Failed to update context run status: {}", err),
            }
        }),
        "context_run_blackboard" => Some({
            if args.len() != 1 {
                return Some("Usage: /context_run_blackboard <run_id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            match client.context_run_blackboard(run_id).await {
                Ok(blackboard) => format!(
                    "Context blackboard {}\n  revision: {}\n  facts: {}\n  decisions: {}\n  open_questions: {}\n  artifacts: {}\n  rolling_summary: {}\n  latest_context_pack: {}",
                    run_id,
                    blackboard.revision,
                    blackboard.facts.len(),
                    blackboard.decisions.len(),
                    blackboard.open_questions.len(),
                    blackboard.artifacts.len(),
                    if blackboard.summaries.rolling.is_empty() { "<empty>" } else { "<present>" },
                    if blackboard.summaries.latest_context_pack.is_empty() { "<empty>" } else { "<present>" }
                ),
                Err(err) => format!("Failed to load context run blackboard: {}", err),
            }
        }),
        "context_run_next" => Some({
            if args.is_empty() {
                return Some("Usage: /context_run_next <run_id> [dry_run]".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            let dry_run = args
                .get(1)
                .map(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes" | "dry"
                    )
                })
                .unwrap_or(false);
            match client.context_run_driver_next(run_id, dry_run).await {
                Ok(next) => format!(
                    "ContextDriver next ({})\n  run: {}\n  dry_run: {}\n  target_status: {}\n  selected_step: {}\n  why_next_step: {}",
                    if dry_run { "preview" } else { "applied" },
                    next.run_id,
                    next.dry_run,
                    format!("{:?}", next.target_status).to_lowercase(),
                    next.selected_step_id.unwrap_or_else(|| "<none>".to_string()),
                    next.why_next_step
                ),
                Err(err) => format!("Failed to run ContextDriver next-step selection: {}", err),
            }
        }),
        "context_run_replay" => Some({
            if args.is_empty() {
                return Some("Usage: /context_run_replay <run_id> [upto_seq]".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            let upto_seq = if args.len() > 1 {
                match args[1].parse::<u64>() {
                    Ok(value) if value > 0 => Some(value),
                    _ => return Some("upto_seq must be a positive integer.".to_string()),
                }
            } else {
                None
            };
            match client.context_run_replay(run_id, upto_seq, Some(true)).await {
                Ok(replay) => format!(
                    "Context replay {}\n  from_checkpoint: {} (seq={})\n  events_applied: {}\n  replay_status: {}\n  persisted_status: {}\n  drift: {} (status={}, why={}, steps={})",
                    replay.run_id,
                    replay.from_checkpoint,
                    replay
                        .checkpoint_seq
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    replay.events_applied,
                    format!("{:?}", replay.replay.status).to_lowercase(),
                    format!("{:?}", replay.persisted.status).to_lowercase(),
                    replay.drift.mismatch,
                    replay.drift.status_mismatch,
                    replay.drift.why_next_step_mismatch,
                    replay.drift.step_count_mismatch
                ),
                Err(err) => format!("Failed to replay context run: {}", err),
            }
        }),
        "context_run_lineage" => Some({
            if args.is_empty() {
                return Some("Usage: /context_run_lineage <run_id> [tail]".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            let tail = if args.len() > 1 {
                match args[1].parse::<usize>() {
                    Ok(value) if value > 0 => Some(value),
                    _ => return Some("tail must be a positive integer.".to_string()),
                }
            } else {
                Some(100)
            };
            match client.context_run_events(run_id, None, tail).await {
                Ok(events) => {
                    let decisions = events
                        .iter()
                        .filter(|event| event.event_type == "meta_next_step_selected")
                        .collect::<Vec<_>>();
                    if decisions.is_empty() {
                        return Some(format!(
                            "No decision lineage events for context run {}.",
                            run_id
                        ));
                    }
                    let lines = decisions
                        .iter()
                        .map(|event| {
                            let why = event
                                .payload
                                .get("why_next_step")
                                .and_then(Value::as_str)
                                .unwrap_or("<missing>");
                            let selected = event
                                .payload
                                .get("selected_step_id")
                                .and_then(Value::as_str)
                                .or_else(|| event.step_id.as_deref())
                                .unwrap_or("-");
                            format!(
                                "- #{} ts={} status={} step={} why={}",
                                event.seq,
                                event.ts_ms,
                                format!("{:?}", event.status).to_lowercase(),
                                selected,
                                why
                            )
                        })
                        .collect::<Vec<_>>();
                    format!(
                        "Context decision lineage ({}):\n{}",
                        run_id,
                        lines.join("\n")
                    )
                }
                Err(err) => format!("Failed to load context run lineage: {}", err),
            }
        }),
        "context_run_sync_tasks" => Some({
            if args.len() != 1 {
                return Some("Usage: /context_run_sync_tasks <run_id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let run_id = args[0];
            let (source_session_id, source_run_id, todos) = match &app.state {
                AppState::Chat {
                    session_id,
                    agents,
                    active_agent_index,
                    tasks,
                    ..
                } => {
                    let mapped = plan_helpers::context_todo_items_from_tasks(tasks);
                    let run_ref = agents
                        .get(*active_agent_index)
                        .and_then(|agent| agent.active_run_id.clone());
                    (Some(session_id.clone()), run_ref, mapped)
                }
                _ => (None, None, Vec::new()),
            };
            if todos.is_empty() {
                return Some("No tasks available to sync.".to_string());
            }
            match client
                .context_run_sync_todos(run_id, todos, true, source_session_id, source_run_id)
                .await
            {
                Ok(run) => format!(
                    "Synced tasks into context run {}.\n  steps: {}\n  status: {}\n  why_next_step: {}",
                    run.run_id,
                    run.steps.len(),
                    format!("{:?}", run.status).to_lowercase(),
                    run.why_next_step.unwrap_or_else(|| "<none>".to_string())
                ),
                Err(err) => format!("Failed to sync tasks into context run: {}", err),
            }
        }),
        "context_run_bind" => Some({
            if args.len() != 1 {
                return Some("Usage: /context_run_bind <run_id|off>".to_string());
            }
            let target = args[0];
            if let AppState::Chat {
                agents,
                active_agent_index,
                ..
            } = &mut app.state
            {
                let Some(agent) = agents.get_mut(*active_agent_index) else {
                    return Some("No active agent.".to_string());
                };
                if target.eq_ignore_ascii_case("off") || target == "-" {
                    agent.bound_context_run_id = None;
                    return Some(format!(
                        "Cleared context-run binding for {}.",
                        agent.agent_id
                    ));
                }
                agent.bound_context_run_id = Some(target.to_string());
                format!(
                    "Bound {} todowrite updates to context run {}.",
                    agent.agent_id, target
                )
            } else {
                "Context-run binding is available in chat mode only.".to_string()
            }
        }),
        "routines" => Some({
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            match client.routines_list().await {
                Ok(routines) => {
                    if routines.is_empty() {
                        return Some("No routines configured.".to_string());
                    }
                    let lines = routines
                        .into_iter()
                        .map(|routine| {
                            let schedule = match routine.schedule {
                                crate::net::client::RoutineSchedule::IntervalSeconds {
                                    seconds,
                                } => format!("interval:{}s", seconds),
                                crate::net::client::RoutineSchedule::Cron { expression } => {
                                    format!("cron:{expression}")
                                }
                            };
                            format!(
                                "- {} [{}] {} ({})",
                                routine.routine_id, routine.name, schedule, routine.entrypoint
                            )
                        })
                        .collect::<Vec<_>>();
                    format!("Routines:\n{}", lines.join("\n"))
                }
                Err(err) => format!("Failed to list routines: {}", err),
            }
        }),
        "routine_create" => Some({
            if args.len() < 3 {
                return Some(
                    "Usage: /routine_create <id> <interval_seconds> <entrypoint>".to_string(),
                );
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let routine_id = args[0].to_string();
            let interval_seconds = match args[1].parse::<u64>() {
                Ok(seconds) if seconds > 0 => seconds,
                _ => return Some("interval_seconds must be a positive integer.".to_string()),
            };
            let entrypoint = args[2..].join(" ");
            let request = crate::net::client::RoutineCreateRequest {
                routine_id: Some(routine_id.clone()),
                name: routine_id.clone(),
                schedule: crate::net::client::RoutineSchedule::IntervalSeconds {
                    seconds: interval_seconds,
                },
                timezone: None,
                misfire_policy: Some(crate::net::client::RoutineMisfirePolicy::RunOnce),
                entrypoint: entrypoint.clone(),
                args: Some(serde_json::json!({})),
                allowed_tools: None,
                output_targets: None,
                creator_type: Some("user".to_string()),
                creator_id: Some("tui".to_string()),
                requires_approval: Some(true),
                external_integrations_allowed: Some(false),
                next_fire_at_ms: None,
            };
            match client.routines_create(request).await {
                Ok(routine) => format!(
                    "Created routine {} ({}s -> {}).",
                    routine.routine_id, interval_seconds, routine.entrypoint
                ),
                Err(err) => format!("Failed to create routine: {}", err),
            }
        }),
        "routine_edit" => Some({
            if args.len() != 2 {
                return Some("Usage: /routine_edit <id> <interval_seconds>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let routine_id = args[0];
            let interval_seconds = match args[1].parse::<u64>() {
                Ok(seconds) if seconds > 0 => seconds,
                _ => return Some("interval_seconds must be a positive integer.".to_string()),
            };
            let request = crate::net::client::RoutinePatchRequest {
                schedule: Some(crate::net::client::RoutineSchedule::IntervalSeconds {
                    seconds: interval_seconds,
                }),
                ..Default::default()
            };
            match client.routines_patch(routine_id, request).await {
                Ok(_) => format!(
                    "Updated routine {} schedule to every {}s.",
                    routine_id, interval_seconds
                ),
                Err(err) => format!("Failed to edit routine: {}", err),
            }
        }),
        "routine_pause" => Some({
            if args.len() != 1 {
                return Some("Usage: /routine_pause <id>".to_string());
            }
            let Some(client) = &app.client else {
                return Some("Engine client not connected.".to_string());
            };
            let routine_id = args[0];
            let request = crate::net::client::RoutinePatchRequest {
                status: Some(crate::net::client::RoutineStatus::Paused),
                ..Default::default()
            };
            match client.routines_patch(routine_id, request).await {
                Ok(_) => format!("Paused routine {}.", routine_id),
                Err(err) => format!("Failed to pause routine: {}", err),
            }
        }),
