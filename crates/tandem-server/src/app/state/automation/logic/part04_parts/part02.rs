fn automation_declared_output_paths_for_run(
    automation: &AutomationV2Spec,
    run_id: &str,
) -> Vec<String> {
    automation_declared_output_paths(automation)
        .into_iter()
        .filter_map(|path| automation_run_scoped_output_path(run_id, &path))
        .collect::<Vec<_>>()
}

pub(crate) async fn clear_automation_declared_outputs(
    state: &AppState,
    automation: &AutomationV2Spec,
    run_id: &str,
) -> anyhow::Result<()> {
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    for output_path in automation_declared_output_paths_for_run(automation, run_id) {
        if let Ok(resolved) = resolve_automation_output_path(&workspace_root, &output_path) {
            if resolved.exists() {
                let _ = std::fs::remove_file(&resolved);
            }
        }
    }
    remove_suspicious_automation_marker_files(&workspace_root);
    Ok(())
}

pub async fn clear_automation_subtree_outputs(
    state: &AppState,
    automation: &AutomationV2Spec,
    run_id: &str,
    node_ids: &std::collections::HashSet<String>,
) -> anyhow::Result<Vec<String>> {
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    let mut cleared = Vec::new();
    for node in &automation.flow.nodes {
        if !node_ids.contains(&node.node_id) {
            continue;
        }
        let Some(output_path) = automation_node_required_output_path(node) else {
            continue;
        };
        let candidates =
            automation_output_path_candidates(&workspace_root, run_id, node, &output_path)?;
        for resolved in candidates {
            if !resolved.exists() || !resolved.is_file() {
                continue;
            }
            std::fs::remove_file(&resolved).map_err(|error| {
                anyhow::anyhow!(
                    "failed to clear subtree output `{}` for automation `{}`: {}",
                    output_path,
                    automation.automation_id,
                    error
                )
            })?;
            if let Some(display) = resolved
                .strip_prefix(&workspace_root)
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
                .filter(|value| !value.is_empty())
            {
                cleared.push(display);
            } else {
                cleared.push(output_path.clone());
            }
        }
    }
    let had_markers = !list_suspicious_automation_marker_files(&workspace_root).is_empty();
    if had_markers {
        remove_suspicious_automation_marker_files(&workspace_root);
    }
    cleared.sort();
    cleared.dedup();
    Ok(cleared)
}

pub(crate) async fn run_automation_node_prompt_with_timeout<F>(
    state: &AppState,
    session_id: &str,
    node: &AutomationFlowNode,
    future: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>>,
{
    let timeout_ms = node
        .timeout_ms
        .filter(|value| *value > 0)
        .unwrap_or_else(|| match automation_output_validator_kind(node) {
            crate::AutomationOutputValidatorKind::StandupUpdate => 120_000,
            crate::AutomationOutputValidatorKind::StructuredJson => 180_000,
            _ => 600_000,
        });
    match tokio::time::timeout(Duration::from_millis(timeout_ms), future).await {
        Ok(result) => result,
        Err(_) => {
            let _ = state.cancellations.cancel(session_id).await;
            anyhow::bail!(
                "automation node `{}` timed out after {} ms",
                node.node_id,
                timeout_ms
            );
        }
    }
}

pub(crate) async fn execute_automation_v2_node(
    state: &AppState,
    run_id: &str,
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    agent: &AutomationAgentProfile,
) -> anyhow::Result<Value> {
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{}` not found", run_id))?;
    let start_cost_usd = run.estimated_cost_usd;
    let start_prompt_tokens = run.prompt_tokens;
    let start_completion_tokens = run.completion_tokens;

    // Phase 5: Check PreexistingArtifactRegistry (MWF-300)
    let prevalidated = {
        let scheduler = state.automation_scheduler.read().await;
        if scheduler
            .preexisting_registry
            .is_artifact_prevalidated(run_id, &node.node_id)
        {
            let path = scheduler
                .preexisting_registry
                .get_prevalidated_path(run_id, &node.node_id)
                .map(|s| s.to_string());
            let digest = scheduler
                .preexisting_registry
                .artifacts
                .get(run_id)
                .and_then(|m| m.get(&node.node_id))
                .map(|a| a.content_digest.clone());
            Some((path, digest))
        } else {
            None
        }
    };

    if let Some((Some(output_path), Some(content_digest))) = prevalidated {
        let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
        let resolved =
            resolve_automation_output_path_for_run(&workspace_root, run_id, &output_path)?;
        if resolved.exists() {
            let current_content = std::fs::read_to_string(&resolved).ok();
            let current_digest = current_content.as_ref().map(|c| sha256_hex(&[c]));
            if current_digest.as_ref() == Some(&content_digest) {
                tracing::info!(
                    run_id = %run_id,
                    node_id = %node.node_id,
                    path = %output_path,
                    "reusing prevalidated artifact from registry (MWF-300)"
                );

                // Build a dummy session to satisfy the output wrapper
                let mut session = Session::new(
                    Some(format!(
                        "Automation {} / {} (Reused)",
                        automation.automation_id, node.node_id
                    )),
                    Some(workspace_root.clone()),
                );
                let session_id = session.id.clone();
                session.project_id = Some(automation_workspace_project_id(&workspace_root));
                session.workspace_root = Some(workspace_root.clone());
                session.messages.push(tandem_types::Message::new(
                    tandem_types::MessageRole::Assistant,
                    vec![tandem_types::MessagePart::Text {
                        text: format!(
                            "Reusing previously validated artifact `{}`.\n\n{{\"status\":\"completed\"}}",
                            output_path
                        ),
                    }],
                ));
                state.storage.save_session(session.clone()).await?;

                let output = node_output::wrap_automation_node_output_with_automation(
                    automation,
                    node,
                    &session,
                    &[],
                    &session_id,
                    Some(run_id),
                    "Reusing previously validated artifact.",
                    Some((output_path, current_content.unwrap())),
                    Some(json!({
                        "accepted_candidate_source": "preexisting_output",
                        "status": "reused_valid"
                    })),
                );
                return Ok(output);
            }
        }
    }

    let attempt = run
        .checkpoint
        .node_attempts
        .get(&node.node_id)
        .copied()
        .unwrap_or(1);
    let workspace_root = resolve_automation_v2_workspace_root(state, automation).await;
    let upstream_inputs = build_automation_v2_upstream_inputs(&run, node, &workspace_root)?;
    let workspace_path = PathBuf::from(&workspace_root);
    if !workspace_path.exists() {
        anyhow::bail!(
            "workspace_root `{}` for automation `{}` does not exist",
            workspace_root,
            automation.automation_id
        );
    }
    if !workspace_path.is_dir() {
        anyhow::bail!(
            "workspace_root `{}` for automation `{}` is not a directory",
            workspace_root,
            automation.automation_id
        );
    }
    let run_started_at_ms = run.started_at_ms.unwrap_or_else(now_ms);
    let required_output_path = automation_effective_required_output_path_for_run(
        automation,
        node,
        run_id,
        run_started_at_ms,
    );
    if let (Some(output_path), Some(payload)) = (
        required_output_path.as_deref(),
        automation_node_inline_artifact_payload(node),
    ) {
        let verified_output =
            write_automation_inline_artifact(&workspace_root, run_id, output_path, &payload)?;
        let mut session = Session::new(
            Some(format!(
                "Automation {} / {}",
                automation.automation_id, node.node_id
            )),
            Some(workspace_root.clone()),
        );
        let session_id = session.id.clone();
        session.project_id = Some(automation_workspace_project_id(&workspace_root));
        session.workspace_root = Some(workspace_root.clone());
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::Text {
                text: format!(
                    "Prepared deterministic workflow artifact `{}` from the node inputs.\n\n{{\"status\":\"completed\"}}",
                    output_path
                ),
            }],
        ));
        state.storage.save_session(session.clone()).await?;
        tracing::info!(
            run_id = %run_id,
            automation_id = %automation.automation_id,
            node_id = %node.node_id,
            output_path = %output_path,
            "automation node used deterministic inline artifact shortcut"
        );
        let output = node_output::wrap_automation_node_output_with_automation(
            automation,
            node,
            &session,
            &[],
            &session_id,
            Some(run_id),
            "Prepared deterministic workflow artifact from inline node inputs.",
            Some(verified_output),
            Some(json!({
                "deterministic_artifact": true,
                "deterministic_source": "node_metadata_inputs",
                "accepted_candidate_source": "verified_output",
                "unmet_requirements": [],
            })),
        );
        return Ok(output);
    }
    let template = if let Some(template_id) = agent.template_id.as_deref().map(str::trim) {
        if template_id.is_empty() {
            None
        } else {
            resolve_automation_agent_template(state, &workspace_root, template_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("agent template `{}` not found", template_id))
                .map(Some)?
        }
    } else {
        None
    };
    let mut session = Session::new(
        Some(format!(
            "Automation {} / {}",
            automation.automation_id, node.node_id
        )),
        Some(workspace_root.clone()),
    );
    let session_id = session.id.clone();
    let project_id = automation_workspace_project_id(&workspace_root);
    session.project_id = Some(project_id.clone());
    session.workspace_root = Some(workspace_root.clone());
    state.storage.save_session(session).await?;

    state.add_automation_v2_session(run_id, &session_id).await;

    let mut allowlist = merge_automation_agent_allowlist(agent, template.as_ref());
    if let Some(mcp_tools) = agent.mcp_policy.allowed_tools.as_ref() {
        allowlist.extend(mcp_tools.clone());
    }
    let mcp_tool_diagnostics = sync_automation_allowed_mcp_servers(
        state,
        node,
        &agent.mcp_policy.allowed_servers,
        &allowlist,
    )
    .await;
    let available_tool_schemas = state.tools.list().await;
    let available_tool_names = available_tool_schemas
        .iter()
        .map(|schema| schema.name.clone())
        .collect::<HashSet<_>>();
    let requested_tools = automation_requested_tools_for_node(
        node,
        &workspace_root,
        allowlist.clone(),
        &available_tool_names,
    );
    let selected_mcp_server_names = mcp_tool_diagnostics
        .get("selected_servers")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let selected_mcp_source = mcp_tool_diagnostics
        .get("selected_source")
        .and_then(Value::as_str)
        .unwrap_or("none")
        .to_string();
    let mut requested_tools = requested_tools;
    requested_tools.extend(automation_requested_server_scoped_mcp_tools(
        node,
        &selected_mcp_server_names,
    ));
    requested_tools.sort();
    requested_tools.dedup();
    let has_selected_mcp_servers_policy =
        !selected_mcp_server_names.is_empty() && selected_mcp_source == "policy";
    let requested_tools =
        automation_add_mcp_list_when_scoped(requested_tools, has_selected_mcp_servers_policy);
    let effective_offered_tools =
        automation_expand_effective_offered_tools(&requested_tools, &available_tool_names);
    let execution_mode = automation_node_execution_mode(node, &workspace_root);
    let mut capability_resolution = automation_resolve_capabilities_with_schemas(
        node,
        execution_mode,
        &effective_offered_tools,
        &available_tool_names,
        &available_tool_schemas,
    );
    if automation_node_requires_email_delivery(node) || has_selected_mcp_servers_policy {
        automation_merge_mcp_capability_diagnostics(
            &mut capability_resolution,
            &mcp_tool_diagnostics,
        );
    }
    let missing_capabilities =
        automation_capability_resolution_missing_capabilities(&capability_resolution);
    let offered_tool_schemas = available_tool_schemas
        .iter()
        .filter(|schema| {
            effective_offered_tools
                .iter()
                .any(|tool| tool == &schema.name)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing_capabilities.is_empty() {
        let offered_tools_summary = if effective_offered_tools.is_empty() {
            "none".to_string()
        } else {
            effective_offered_tools.join(", ")
        };
        let selected_servers_summary = {
            let servers = mcp_tool_diagnostics
                .get("selected_servers")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if servers.is_empty() {
                "none".to_string()
            } else {
                servers.join(", ")
            }
        };
        let registered_tools_summary = {
            let tools = mcp_tool_diagnostics
                .get("registered_tools")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if tools.is_empty() {
                "none".to_string()
            } else {
                tools.join(", ")
            }
        };
        let detail = format!(
            "required automation capabilities were not offered after MCP/tool sync: {}. Offered tools: {}. Selected MCP servers: {}. Registered MCP tools after sync: {}.",
            missing_capabilities.join(", "),
            offered_tools_summary,
            selected_servers_summary,
            registered_tools_summary
        );
        let mut output =
            crate::automation_v2::executor::build_node_execution_error_output_with_category(
                node,
                &detail,
                false,
                "tool_resolution_failed",
            );
        if let Some(object) = output.as_object_mut() {
            object.insert(
                "tool_telemetry".to_string(),
                automation_initialized_attempt_tool_telemetry(
                    &requested_tools,
                    &capability_resolution,
                ),
            );
            object.insert(
                "capability_resolution".to_string(),
                capability_resolution.clone(),
            );
        }
        return Ok(output);
    }
    state
        .set_automation_v2_session_mcp_servers(&session_id, selected_mcp_server_names.clone())
        .await;
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, requested_tools.clone())
        .await;
    state
        .engine_loop
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    let model = resolve_automation_agent_model(state, agent, template.as_ref()).await;
    let runtime_values = automation_prompt_runtime_values(run.started_at_ms);
    let preexisting_output = required_output_path
        .as_deref()
        .and_then(|output_path| {
            automation_output_path_candidates(&workspace_root, run_id, node, output_path)
                .ok()
                .and_then(|candidates| {
                    candidates
                        .into_iter()
                        .find(|candidate| candidate.exists() && candidate.is_file())
                })
        })
        .and_then(|resolved| std::fs::read_to_string(resolved).ok());
    let read_only_source_snapshot = automation_read_only_file_snapshot_for_node(
        &workspace_root,
        &enforcement::automation_node_required_source_read_paths_for_automation(
            automation,
            node,
            &workspace_root,
            Some(&runtime_values),
        ),
    );
    let mut read_only_source_snapshot_rollback =
        ReadOnlySourceSnapshotRollback::armed(&workspace_root, &read_only_source_snapshot);
    let workspace_snapshot_before = automation_workspace_root_file_snapshot(&workspace_root);
    let standup_report_path =
        if is_agent_standup_automation(automation) && node.node_id == "standup_synthesis" {
            resolve_standup_report_path_for_run(automation, run_started_at_ms)
        } else {
            None
        };
    // P1: Delta-aware standup — read the most recent previous standup report and inject it.
    // This gives both participants and the coordinator awareness of what was already reported,
    // allowing them to report only new progress rather than re-discovering the same workspace state.
    let previous_standup_context: Option<String> = if is_agent_standup_automation(automation) {
        let report_path_template = resolve_standup_report_path_template(automation);
        let run_ts = run.started_at_ms.unwrap_or_else(now_ms);
        let previous_report = report_path_template.and_then(|template| {
            // Try up to 7 days back to find the most recent previous report
            for days_back in 1u64..=7 {
                let previous_ts = run_ts.saturating_sub(days_back * 24 * 60 * 60 * 1000);
                let candidate_path = if template.contains("{{date}}") {
                    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
                        previous_ts as i64,
                    )
                    .unwrap_or_else(chrono::Utc::now)
                    .format("%Y-%m-%d")
                    .to_string();
                    template.replace("{{date}}", &date)
                } else {
                    break;
                };
                if let Ok(resolved) =
                    resolve_automation_output_path(&workspace_root, &candidate_path)
                {
                    if resolved.is_file() {
                        if let Ok(content) = std::fs::read_to_string(&resolved) {
                            let trimmed = content.trim();
                            if !trimmed.is_empty() {
                                return Some(format!(
                                    "Previous Standup Report ({}):\n{}\n\nReport only NEW progress since the above. Do not repeat items already listed in the previous standup.",
                                    candidate_path,
                                    trimmed
                                ));
                            }
                        }
                    }
                }
            }
            None
        });
        previous_report
    } else {
        None
    };
    let knowledge_preflight =
        automation_knowledge_preflight(state, automation, node, run_id, &project_id).await;
    let (approved_learning_ids, workflow_learning_context) = state
        .workflow_learning_context_for_automation_node(automation, node)
        .await;
    let knowledge_context = {
        let base = knowledge_preflight.as_ref().and_then(|preflight| {
            if !preflight.is_reusable() {
                return None;
            }
            let rendered = preflight.format_for_injection();
            if rendered.trim().is_empty() {
                None
            } else {
                Some(rendered)
            }
        });
        match (base, workflow_learning_context, previous_standup_context) {
            (Some(base), Some(learning), Some(prev)) => {
                Some(format!("{base}\n\n{learning}\n\n{prev}"))
            }
            (Some(base), Some(learning), None) => Some(format!("{base}\n\n{learning}")),
            (Some(base), None, Some(prev)) => Some(format!("{base}\n\n{prev}")),
            (None, Some(learning), Some(prev)) => Some(format!("{learning}\n\n{prev}")),
            (Some(base), None, None) => Some(base),
            (None, Some(learning), None) => Some(learning),
            (None, None, Some(prev)) => Some(prev),
            (None, None, None) => None,
        }
    };
    if !approved_learning_ids.is_empty() {
        let _ = state
            .record_automation_v2_run_learning_usage(run_id, &approved_learning_ids)
            .await;
    }
    let max_attempts = automation_node_max_attempts(node);
    let mut prompt = render_automation_v2_prompt_with_options(
        automation,
        &workspace_root,
        run_id,
        node,
        attempt,
        agent,
        &upstream_inputs,
        &requested_tools,
        template
            .as_ref()
            .and_then(|value| value.system_prompt.as_deref()),
        standup_report_path.as_deref(),
        if is_agent_standup_automation(automation) {
            Some(project_id.as_str())
        } else {
            None
        },
        AutomationPromptRenderOptions {
            summary_only_upstream: false,
            knowledge_context: knowledge_context.clone(),
            runtime_values: Some(runtime_values.clone()),
        },
    );
    let preserve_full_upstream_inputs = automation_node_preserves_full_upstream_inputs(node);
    let mut preflight = build_automation_prompt_preflight(
        &prompt,
        &effective_offered_tools,
        &offered_tool_schemas,
        execution_mode,
        &capability_resolution,
        "standard",
        false,
    );
    if automation_preflight_should_degrade(&preflight) && !upstream_inputs.is_empty() {
        if preserve_full_upstream_inputs {
            preflight = build_automation_prompt_preflight(
                &prompt,
                &effective_offered_tools,
                &offered_tool_schemas,
                execution_mode,
                &capability_resolution,
                "full_upstream_preserved",
                true,
            );
        } else {
            prompt = render_automation_v2_prompt_with_options(
                automation,
                &workspace_root,
                run_id,
                node,
                attempt,
                agent,
                &upstream_inputs,
                &requested_tools,
                template
                    .as_ref()
                    .and_then(|value| value.system_prompt.as_deref()),
                standup_report_path.as_deref(),
                if is_agent_standup_automation(automation) {
                    Some(project_id.as_str())
                } else {
                    None
                },
                AutomationPromptRenderOptions {
                    summary_only_upstream: true,
                    knowledge_context: knowledge_context.clone(),
                    runtime_values: Some(runtime_values.clone()),
                },
            );
            preflight = build_automation_prompt_preflight(
                &prompt,
                &effective_offered_tools,
                &offered_tool_schemas,
                execution_mode,
                &capability_resolution,
                "summary_only_upstream",
                true,
            );
        }
    }
    if let Some(repair_brief) = render_automation_repair_brief(
        node,
        run.checkpoint.node_outputs.get(&node.node_id),
        attempt,
        max_attempts,
        Some(run_id),
    ) {
        prompt.push_str("\n\n");
        prompt.push_str(&repair_brief);
    }
    let req = SendMessageRequest {
        parts: vec![MessagePartInput::Text { text: prompt }],
        model: model.clone(),
        agent: None,
        tool_mode: Some(ToolMode::Required),
        tool_allowlist: Some(requested_tools.clone()),
        context_mode: None,
        write_required: required_output_path.as_ref().map(|_| true),
        prewrite_requirements: automation_node_prewrite_requirements(node, &requested_tools),
    };
    let result = run_automation_node_prompt_with_timeout(
        state,
        &session_id,
        node,
        state.engine_loop.run_prompt_async_with_context(
            session_id.clone(),
            req,
            Some(format!("automation-v2:{run_id}")),
        ),
    )
    .await;

    state
        .engine_loop
        .clear_session_allowed_tools(&session_id)
        .await;
    state
        .engine_loop
        .clear_session_auto_approve_permissions(&session_id)
        .await;
    state
        .clear_automation_v2_session_mcp_servers(&session_id)
        .await;
    state.clear_automation_v2_session(run_id, &session_id).await;

    if let Err(error) = result {
        return Err(error);
    }
    let expect_tool_activity = !requested_tools.is_empty();
    let session = load_automation_session_after_run(state, &session_id, expect_tool_activity)
        .await
        .ok_or_else(|| anyhow::anyhow!("automation session `{}` missing after run", session_id))?;
    let session_text = extract_session_text_output(&session);
    let verified_output = if let Some(output_path) = required_output_path.as_deref() {
        let resolution = reconcile_automation_resolve_verified_output_path(
            &session,
            &workspace_root,
            run_id,
            node,
            output_path,
            250,
            25,
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "required output `{}` was not created for node `{}`",
                output_path,
                node.node_id
            )
        })?;
        let resolved = resolution.path.clone();
        if !resolved.is_file() {
            anyhow::bail!(
                "required output `{}` for node `{}` is not a file",
                output_path,
                node.node_id
            );
        }
        let file_text = std::fs::read_to_string(&resolved).map_err(|error| {
            anyhow::anyhow!(
                "required output `{}` for node `{}` could not be read: {}",
                output_path,
                node.node_id,
                error
            )
        })?;
        let display_path = resolved
            .strip_prefix(&workspace_root)
            .ok()
            .and_then(|value| value.to_str().map(str::to_string))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| output_path.to_string());
        Some((display_path, file_text, resolution))
    } else {
        None
    };
    let tool_telemetry = summarize_automation_tool_activity(node, &session, &requested_tools);
    let mut tool_telemetry = tool_telemetry;
    let verified_output_resolution = verified_output
        .as_ref()
        .map(|(_, _, resolution)| resolution.clone());
    let verified_output_for_evidence = verified_output
        .as_ref()
        .map(|(path, text, _)| (path.clone(), text.clone()));
    let base_attempt_evidence = node_output::build_automation_attempt_evidence(
        node,
        attempt,
        &session,
        &session_id,
        &workspace_root,
        &tool_telemetry,
        &preflight,
        &capability_resolution,
        required_output_path.as_deref(),
        verified_output_resolution.as_ref(),
        verified_output_for_evidence.as_ref(),
    );
    if let Some(object) = tool_telemetry.as_object_mut() {
        object.insert("preflight".to_string(), preflight.clone());
        object.insert(
            "capability_resolution".to_string(),
            capability_resolution.clone(),
        );
        object.insert(
            "verified_output_materialized_by_current_attempt".to_string(),
            json!(verified_output_resolution
                .as_ref()
                .map(|resolution| resolution.materialized_by_current_attempt)
                .unwrap_or(false)),
        );
        object.insert(
            "attempt_evidence".to_string(),
            base_attempt_evidence.clone(),
        );
    }
    let upstream_evidence = if automation_node_uses_upstream_validation_evidence(node) {
        Some(
            collect_automation_upstream_research_evidence(
                state,
                automation,
                &run,
                node,
                &workspace_root,
            )
            .await,
        )
    } else {
        None
    };
    let verified_output = verified_output.map(|(path, text, _)| (path, text));
    let (verified_output, mut artifact_validation, artifact_rejected_reason) =
        validate_automation_artifact_output_with_context(
            automation,
            node,
            &session,
            &workspace_root,
            Some(run_id.as_ref()),
            Some(&runtime_values),
            &session_text,
            &tool_telemetry,
            preexisting_output.as_deref(),
            verified_output,
            &workspace_snapshot_before,
            upstream_evidence.as_ref(),
            Some(&read_only_source_snapshot),
        );
    let _ = artifact_rejected_reason;
    if let Some(promoted_from) = verified_output_resolution
        .as_ref()
        .and_then(|resolution| resolution.legacy_workspace_artifact_promoted_from.as_ref())
    {
        if let Some(object) = artifact_validation.as_object_mut() {
            object.insert(
                "legacy_workspace_artifact_promoted".to_string(),
                json!(true),
            );
            object.insert(
                "legacy_workspace_artifact_promoted_from".to_string(),
                json!(promoted_from.to_string_lossy().to_string()),
            );
            object
                .entry("accepted_candidate_source".to_string())
                .or_insert_with(|| json!("legacy_workspace_artifact_promoted"));
        }
    }

    let editorial_publish_block_reason = state
        .get_automation_v2_run(run_id)
        .await
        .and_then(|run| automation_publish_editorial_block_reason(&run, node));
    if let Some(reason) = editorial_publish_block_reason.as_ref() {
        if let Some(object) = artifact_validation.as_object_mut() {
            let unmet = object
                .entry("unmet_requirements".to_string())
                .or_insert_with(|| json!([]));
            if let Some(rows) = unmet.as_array_mut() {
                if !rows
                    .iter()
                    .any(|value| value.as_str() == Some("editorial_clearance_required"))
                {
                    rows.push(json!("editorial_clearance_required"));
                }
            }
            object
                .entry("semantic_block_reason".to_string())
                .or_insert_with(|| Value::String(reason.clone()));
        }
    }
    let artifact_publication = if artifact_validation
        .get("semantic_block_reason")
        .and_then(Value::as_str)
        .is_none()
    {
        if let Some(verified_output) = verified_output.as_ref() {
            if let Some(spec) = automation_node_publish_spec(node) {
                Some(
                    publish_automation_verified_output(
                        &workspace_root,
                        automation,
                        run_id,
                        node,
                        verified_output,
                        &spec,
                    )
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "durable publication failed for node `{}` after validating `{}`: {}",
                            node.node_id,
                            verified_output.0,
                            error
                        )
                    })?,
                )
            } else if !automation.output_targets.is_empty() {
                Some(
                    publish_automation_verified_outputs(
                        &workspace_root,
                        automation,
                        run_id,
                        node,
                        verified_output,
                    )
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "durable publication failed for node `{}` after validating `{}`: {}",
                            node.node_id,
                            verified_output.0,
                            error
                        )
                    })?,
                )
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    if let Some(publication) = artifact_publication.clone() {
        if let Some(object) = artifact_validation.as_object_mut() {
            object.insert("artifact_publication".to_string(), publication);
        }
    }
    let (receipt_status, receipt_blocked_reason, receipt_approved) =
        node_output::detect_automation_node_status(
            node,
            &session_text,
            verified_output.as_ref(),
            &tool_telemetry,
            Some(&artifact_validation),
        );
    let receipt_blocker_category = node_output::detect_automation_blocker_category(
        node,
        &receipt_status,
        receipt_blocked_reason.as_deref(),
        &tool_telemetry,
        Some(&artifact_validation),
    );
    let receipt_fallback_used =
        automation_attempt_uses_legacy_fallback(&session_text, Some(&artifact_validation));
    let receipt_validator_summary = node_output::build_automation_validator_summary(
        automation_output_validator_kind(node),
        &receipt_status,
        receipt_blocked_reason.as_deref(),
        Some(&artifact_validation),
    );
    let receipt_attempt_evidence = tool_telemetry
        .get("attempt_evidence")
        .cloned()
        .map(|value| {
            node_output::augment_automation_attempt_evidence_with_validation(
                &value,
                Some(&artifact_validation),
                verified_output.as_ref(),
                artifact_validation
                    .get("accepted_candidate_source")
                    .and_then(Value::as_str),
                receipt_blocker_category.as_deref(),
                receipt_fallback_used,
                node_output::automation_backend_actionability_state(&receipt_status),
            )
        });
    let receipt_telemetry_summary = json!({
        "receipt_kind": "tool_telemetry_summary",
        "automation_id": automation.automation_id,
        "automation_run_id": run_id,
        "context_run_id": format!("automation-v2-{run_id}"),
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "preflight": preflight.clone(),
        "capability_resolution": capability_resolution.clone(),
        "tool_call_counts": tool_telemetry.get("tool_call_counts").cloned().unwrap_or_else(|| json!({})),
        "web_research_used": tool_telemetry.get("web_research_used").cloned().unwrap_or_else(|| json!(false)),
        "web_research_succeeded": tool_telemetry.get("web_research_succeeded").cloned().unwrap_or_else(|| json!(false)),
        "latest_web_research_failure": tool_telemetry.get("latest_web_research_failure").cloned().unwrap_or(Value::Null),
        "email_delivery_attempted": tool_telemetry.get("email_delivery_attempted").cloned().unwrap_or_else(|| json!(false)),
        "email_delivery_succeeded": tool_telemetry.get("email_delivery_succeeded").cloned().unwrap_or_else(|| json!(false)),
        "latest_email_delivery_failure": tool_telemetry.get("latest_email_delivery_failure").cloned().unwrap_or(Value::Null),
    });
    let receipt_attempt_summary = json!({
        "receipt_kind": "attempt_summary",
        "automation_id": automation.automation_id,
        "automation_run_id": run_id,
        "context_run_id": format!("automation-v2-{run_id}"),
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "status": receipt_status,
        "approved": receipt_approved,
        "blocked_reason": receipt_blocked_reason,
        "blocker_category": receipt_blocker_category,
        "fallback_used": receipt_fallback_used,
        "attempt_evidence": receipt_attempt_evidence,
    });
    let receipt_validation_summary = json!({
        "receipt_kind": "validation_summary",
        "automation_id": automation.automation_id,
        "automation_run_id": run_id,
        "context_run_id": format!("automation-v2-{run_id}"),
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "validator_summary": receipt_validator_summary,
    });
    let mut receipt_events = collect_automation_attempt_receipt_events(
        automation,
        run_id,
        node,
        attempt,
        &session_id,
        &session,
        verified_output.as_ref(),
        verified_output_resolution.as_ref(),
        required_output_path.as_deref(),
        Some(&artifact_validation),
    );
    receipt_events.extend(vec![
        AutomationAttemptReceiptEventInput {
            event_type: "attempt_summary".to_string(),
            payload: receipt_attempt_summary,
        },
        AutomationAttemptReceiptEventInput {
            event_type: "tool_telemetry_summary".to_string(),
            payload: receipt_telemetry_summary,
        },
        AutomationAttemptReceiptEventInput {
            event_type: "validation_summary".to_string(),
            payload: receipt_validation_summary,
        },
    ]);
    let receipt_root = receipts::automation_attempt_receipts_root();
    let receipt_ledger = match append_automation_attempt_receipts(
        &receipt_root,
        run_id,
        &node.node_id,
        attempt,
        &session_id,
        &receipt_events,
    )
    .await
    {
        Ok(summary) => Some(serde_json::to_value(summary)?),
        Err(error) => {
            tracing::warn!(
                run_id = %run_id,
                node_id = %node.node_id,
                attempt = attempt,
                error = %error,
                "failed to append automation attempt receipt ledger"
            );
            None
        }
    };
    let receipt_timeline = receipt_ledger
        .as_ref()
        .and_then(|ledger| ledger.get("path").and_then(Value::as_str))
        .map(PathBuf::from);
    let receipt_timeline = match receipt_timeline {
        Some(path) => receipts::read_automation_attempt_receipt_records(&path)
            .await
            .ok()
            .map(|records| {
                json!({
                    "record_count": records.len(),
                    "records": records,
                })
            }),
        None => None,
    };
    let attempt_forensic_record = json!({
        "version": 1,
        "automation_id": automation.automation_id,
        "automation_run_id": run_id,
        "context_run_id": format!("automation-v2-{run_id}"),
        "node_id": node.node_id,
        "attempt": attempt,
        "session_id": session_id,
        "status": receipt_status,
        "final_backend_actionability_state": node_output::automation_backend_actionability_state(&receipt_status),
        "approved": receipt_approved,
        "blocked_reason": receipt_blocked_reason,
        "blocker_category": receipt_blocker_category,
        "fallback_used": receipt_fallback_used,
        "preflight": preflight.clone(),
        "capability_resolution": capability_resolution.clone(),
        "validator_summary": receipt_validator_summary,
        "attempt_evidence": receipt_attempt_evidence.clone(),
        "receipt_ledger": receipt_ledger.clone(),
        "receipt_timeline": receipt_timeline.clone(),
    });
    let attempt_forensic_record_path = match receipts::persist_automation_attempt_forensic_record(
        &workspace_root,
        run_id,
        &node.node_id,
        attempt,
        &attempt_forensic_record,
    )
    .await
    {
        Ok(path) => Some(path.to_string_lossy().to_string()),
        Err(error) => {
            tracing::warn!(
                run_id = %run_id,
                node_id = %node.node_id,
                attempt = attempt,
                error = %error,
                "failed to persist automation attempt forensic record"
            );
            None
        }
    };
    let external_actions = if editorial_publish_block_reason.is_some() {
        Vec::new()
    } else {
        record_automation_external_actions_for_session(
            state,
            run_id,
            automation,
            node,
            attempt,
            &session_id,
            &session,
        )
        .await?
    };
    let mut output = wrap_automation_node_output_with_automation(
        automation,
        node,
        &session,
        &requested_tools,
        &session_id,
        Some(run_id),
        &session_text,
        verified_output,
        Some(artifact_validation),
    );
    let run_after = state.get_automation_v2_run(run_id).await.unwrap_or(run);
    let cost_usd_delta = run_after.estimated_cost_usd - start_cost_usd;
    let prompt_tokens_delta = run_after.prompt_tokens.saturating_sub(start_prompt_tokens);
    let completion_tokens_delta = run_after
        .completion_tokens
        .saturating_sub(start_completion_tokens);
    let budget_limit_reached = automation
        .execution
        .max_total_cost_usd
        .map(|max| run_after.estimated_cost_usd >= max)
        .unwrap_or(false);
    let cost_provenance = automation_step_cost_provenance(
        &node.node_id,
        model.map(|m| m.model_id.clone()),
        prompt_tokens_delta,
        completion_tokens_delta,
        cost_usd_delta,
        run_after.estimated_cost_usd,
        budget_limit_reached,
    );
    if let Some(object) = output.as_object_mut() {
        object.insert("cost_provenance".to_string(), cost_provenance);
        if let Some(knowledge_preflight) = knowledge_preflight.as_ref() {
            object.insert(
                "knowledge_preflight".to_string(),
                serde_json::to_value(knowledge_preflight)?,
            );
        }
        if let Some(publication) = artifact_publication {
            object.insert("artifact_publication".to_string(), publication);
        }
        if let Some(receipt_timeline) = receipt_timeline.clone() {
            object.insert("receipt_timeline".to_string(), receipt_timeline);
        }
        if let Some(receipt_ledger) = receipt_ledger {
            if let Some(attempt_evidence) = object
                .get_mut("attempt_evidence")
                .and_then(Value::as_object_mut)
            {
                attempt_evidence.insert("receipt_ledger".to_string(), receipt_ledger);
                if let Some(receipt_timeline) = receipt_timeline {
                    attempt_evidence.insert("receipt_timeline".to_string(), receipt_timeline);
                }
            }
        }
        if let Some(path) = attempt_forensic_record_path.clone() {
            object.insert(
                "attempt_forensic_record_path".to_string(),
                json!(path.clone()),
            );
            if let Some(attempt_evidence) = object
                .get_mut("attempt_evidence")
                .and_then(Value::as_object_mut)
            {
                attempt_evidence.insert("forensic_record_path".to_string(), json!(path));
            }
        }
        if !external_actions.is_empty() {
            object.insert(
                "external_actions".to_string(),
                serde_json::to_value(&external_actions)?,
            );
        }

        // --- A. Standup coordinator assessment scoring ---
        // Reuses assess_artifact_candidate() from assessment.rs to score the
        // coordinator's synthesis report. Records score + breakdown as metadata.
        // Does NOT hard-block. Soft warning only for low-quality outputs.
        //
        // Score thresholds (informational, not enforcement gates):
        //   < 0   : effectively empty/broken
        //   < 500 : weak — warning logged + standup_quality_warning flag set
        //   >= 500: acceptable
        //   >= 2000: strong (substantive flag set by assess_artifact_candidate)
        if is_agent_standup_automation(automation) && node.node_id == "standup_synthesis" {
            let report_text = object
                .get("content")
                .and_then(|c| c.get("text").and_then(Value::as_str))
                .or_else(|| {
                    object
                        .get("content")
                        .and_then(|c| c.get("raw_assistant_text").and_then(Value::as_str))
                })
                .unwrap_or(&session_text);
            let read_paths = tool_telemetry
                .get("read_paths")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let assessment = assess_artifact_candidate(
                node,
                &workspace_root,
                "session_write",
                report_text,
                &read_paths,
                &[],
                &[],
                &[],
            );
            let assessment_summary = assessment::artifact_candidate_summary(&assessment, true);
            object.insert("standup_assessment".to_string(), assessment_summary);
            if assessment.score < 500 {
                object.insert("standup_quality_warning".to_string(), json!(true));
                tracing::warn!(
                    run_id = %run_id,
                    node_id = %node.node_id,
                    score = assessment.score,
                    substantive = assessment.substantive,
                    placeholder_like = assessment.placeholder_like,
                    "standup coordinator output scored below warning threshold (500); \
                     report may be low-quality"
                );
            }

            // --- B. Operator-facing standup run receipt ---
            // Generates a JSON receipt beside the standup report using existing
            // node_outputs, node_attempts, lifecycle_history, and assessment data.
            // The receipt path is derived from the report path by inserting a
            // "receipt-" prefix on the filename, e.g.:
            //   docs/standups/2026-04-05.md -> docs/standups/receipt-2026-04-05.json
            if let Some(report_path) = standup_report_path.as_deref() {
                if let Some(receipt_json) = build_standup_run_receipt(
                    &run_after,
                    automation,
                    run_id,
                    report_path,
                    &assessment,
                ) {
                    let receipt_path = standup_receipt_path_for_report(report_path);
                    let abs_receipt = PathBuf::from(&workspace_root).join(&receipt_path);
                    if let Some(parent) = abs_receipt.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match serde_json::to_string_pretty(&receipt_json) {
                        Ok(content) => match std::fs::write(&abs_receipt, &content) {
                            Ok(()) => {
                                object.insert(
                                    "standup_receipt_path".to_string(),
                                    json!(receipt_path),
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    run_id = %run_id,
                                    receipt_path = %receipt_path,
                                    error = %err,
                                    "failed to write standup run receipt"
                                );
                            }
                        },
                        Err(err) => {
                            tracing::warn!(
                                run_id = %run_id,
                                error = %err,
                                "failed to serialize standup run receipt"
                            );
                        }
                    }
                }
            }
        }
    }
    read_only_source_snapshot_rollback.disarm();
    Ok(output)
}
