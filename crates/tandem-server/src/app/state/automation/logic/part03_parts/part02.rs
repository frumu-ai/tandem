fn git_diff_summary_for_paths(workspace_root: &str, paths: &[String]) -> Option<Value> {
    if paths.is_empty() || !workspace_has_git_repo(workspace_root) {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(workspace_root)
        .arg("diff")
        .arg("--stat")
        .arg("--");
    for path in paths {
        cmd.arg(path);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if summary.is_empty() {
        None
    } else {
        Some(json!({
            "stat": summary
        }))
    }
}

#[cfg(test)]
pub(crate) fn validate_automation_artifact_output(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
) -> (Option<(String, String)>, Value, Option<String>) {
    validate_automation_artifact_output_with_upstream(
        node,
        session,
        workspace_root,
        None,
        session_text,
        tool_telemetry,
        preexisting_output,
        verified_output,
        workspace_snapshot_before,
        None,
    )
}

pub(crate) fn validate_automation_artifact_output_with_upstream(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    run_id: Option<&str>,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
    upstream_evidence: Option<&AutomationUpstreamEvidence>,
) -> (Option<(String, String)>, Value, Option<String>) {
    let automation = AutomationV2Spec {
        automation_id: "validation".to_string(),
        name: "validation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Draft,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: None,
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "validation".to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    validate_automation_artifact_output_with_context(
        &automation,
        node,
        session,
        workspace_root,
        run_id,
        None,
        session_text,
        tool_telemetry,
        preexisting_output,
        verified_output,
        workspace_snapshot_before,
        upstream_evidence,
        None,
    )
}

pub(crate) fn validate_automation_artifact_output_with_context(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    run_id: Option<&str>,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
    upstream_evidence: Option<&AutomationUpstreamEvidence>,
    read_only_source_snapshot: Option<&std::collections::BTreeMap<String, Vec<u8>>>,
) -> (Option<(String, String)>, Value, Option<String>) {
    let suspicious_after = list_suspicious_automation_marker_files(workspace_root);
    let undeclared_files_created = suspicious_after
        .iter()
        .filter(|name| !workspace_snapshot_before.contains((*name).as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut auto_cleaned = false;
    if !suspicious_after.is_empty() {
        remove_suspicious_automation_marker_files(workspace_root);
        auto_cleaned = true;
    }

    let enforcement = automation_node_output_enforcement(node);
    let validator_kind = automation_output_validator_kind(node);
    let execution_policy = automation_node_execution_policy(node, workspace_root);
    let must_write_files =
        automation_node_must_write_files_for_automation(automation, node, runtime_values);
    let mutation_summary = session_file_mutation_summary(session, workspace_root);
    let verification_summary = session_verification_summary(node, session);
    let touched_files = mutation_summary
        .get("touched_files")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mutation_tool_by_file = mutation_summary
        .get("mutation_tool_by_file")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut rejected_reason = if undeclared_files_created.is_empty() {
        None
    } else {
        Some(format!(
            "undeclared marker files created: {}",
            undeclared_files_created.join(", ")
        ))
    };
    let mut semantic_block_reason = None::<String>;
    let mut unmet_requirements = Vec::<String>::new();
    let mut read_only_source_mutations = Vec::<Value>::new();
    if let Some(snapshot) = read_only_source_snapshot {
        let workspace_root_path = PathBuf::from(workspace_root);
        for (path, before) in snapshot {
            let resolved = workspace_root_path.join(path);
            let mutation = if !resolved.is_file() {
                Some(json!({
                    "path": path,
                    "issue": "deleted",
                }))
            } else {
                match std::fs::read(&resolved) {
                    Ok(after) if after == *before => None,
                    Ok(_after) => Some(json!({
                        "path": path,
                        "issue": "modified",
                    })),
                    Err(_) => Some(json!({
                        "path": path,
                        "issue": "read_failed_after_run",
                    })),
                }
            };

            if let Some(entry) = mutation {
                read_only_source_mutations.push(entry);
                if let Some(parent) = resolved.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&resolved, before);
            }
        }
        if !read_only_source_mutations.is_empty() {
            let mutation_paths = read_only_source_mutations
                .iter()
                .filter_map(|value| value.get("path").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();
            unmet_requirements.push("read_only_source_mutations".to_string());
            if semantic_block_reason.is_none() {
                semantic_block_reason = Some(
                    "artifact blocked by attempted mutation of read-only source-of-truth input files"
                        .to_string(),
                );
            }
            if rejected_reason.is_none() {
                rejected_reason = Some(format!(
                    "read-only source-of-truth mutation detected: {}",
                    mutation_paths.join(", ")
                ));
            }
        }
    }
    let verified_output_materialized = verified_output.as_ref().is_some_and(|value| {
        tool_telemetry
            .get("verified_output_materialized_by_current_attempt")
            .and_then(Value::as_bool)
            .unwrap_or(true)
            && automation_verified_output_differs_from_preexisting(preexisting_output, value)
    });
    let mut accepted_output = verified_output;
    let mut recovered_from_session_write = false;
    let quality_mode_resolution = enforcement::automation_node_quality_mode_resolution(node);
    let mut validation_basis = json!({
        "authority": "filesystem_and_receipts",
        "quality_mode": quality_mode_resolution.effective.stable_key(),
        "requested_quality_mode": quality_mode_resolution
            .requested
            .map(|mode| mode.stable_key()),
        "legacy_quality_rollback_enabled": quality_mode_resolution.legacy_rollback_enabled,
    });
    let current_read_paths = session_read_paths(session, workspace_root);
    let current_discovered_relevant_paths =
        session_discovered_relevant_paths(session, workspace_root);
    let use_upstream_evidence = automation_node_uses_upstream_validation_evidence(node);
    let upstream_read_paths = upstream_evidence
        .map(|evidence| evidence.read_paths.clone())
        .unwrap_or_default();
    let required_source_read_paths =
        enforcement::automation_node_required_source_read_paths_for_automation(
            automation,
            node,
            workspace_root,
            runtime_values,
        );
    let missing_required_source_read_paths = required_source_read_paths
        .iter()
        .filter(|path| {
            let current_read = current_read_paths.iter().any(|read| read == *path);
            let upstream_read =
                use_upstream_evidence && upstream_read_paths.iter().any(|read| read == *path);
            !current_read && !upstream_read
        })
        .cloned()
        .collect::<Vec<_>>();
    if let Some(object) = validation_basis.as_object_mut() {
        object.insert(
            "required_source_read_paths".to_string(),
            json!(required_source_read_paths),
        );
        object.insert(
            "missing_required_source_read_paths".to_string(),
            json!(missing_required_source_read_paths),
        );
    }
    let explicit_input_files =
        automation_node_effective_input_files_for_automation(automation, node, runtime_values);
    let explicit_output_files =
        automation_node_effective_output_files_for_automation(automation, node, runtime_values);
    let mut read_paths = current_read_paths.clone();
    let mut discovered_relevant_paths = if use_upstream_evidence {
        let mut paths = Vec::new();
        if let Some(upstream) = upstream_evidence {
            read_paths.extend(upstream.read_paths.clone());
            paths.extend(upstream.discovered_relevant_paths.clone());
        }
        paths
    } else {
        current_discovered_relevant_paths.clone()
    };
    if !explicit_input_files.is_empty() {
        discovered_relevant_paths = explicit_input_files.clone();
    }
    read_paths.sort();
    read_paths.dedup();
    discovered_relevant_paths.sort();
    discovered_relevant_paths.dedup();
    let mut reviewed_paths_backed_by_read = Vec::<String>::new();
    let mut unreviewed_relevant_paths = Vec::<String>::new();
    let mut repair_attempted = false;
    let mut repair_succeeded = false;
    let mut citation_count = 0usize;
    let mut web_sources_reviewed_present = false;
    let mut heading_count = 0usize;
    let mut paragraph_count = 0usize;
    let mut artifact_candidates = Vec::<Value>::new();
    let mut accepted_candidate_source = None::<String>;
    let mut blocked_handoff_cleanup_action = None::<String>;
    let mcp_grounded_citations_artifact =
        automation_node_is_mcp_grounded_citations_artifact(node, tool_telemetry);
    let execution_mode = execution_policy
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("artifact_write");
    let requires_current_attempt_output = execution_mode == "artifact_write"
        && automation_node_required_output_path(node).is_some()
        && !automation_node_allows_preexisting_output_reuse(node);
    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    let enforcement_requires_evidence = !enforcement.required_tools.is_empty()
        || !enforcement.required_evidence.is_empty()
        || !enforcement.required_sections.is_empty()
        || !enforcement.prewrite_gates.is_empty();
    let parsed_status = parse_status_json(session_text);
    let structured_handoff = if handoff_only_structured_json {
        extract_structured_handoff_json(session_text)
    } else {
        None
    };
    let repair_exhausted_hint = parsed_status
        .as_ref()
        .and_then(|value| value.get("repairExhausted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if rejected_reason.is_none() && matches!(execution_mode, "git_patch" | "filesystem_patch") {
        let unsafe_raw_write_paths = touched_files
            .iter()
            .filter(|path| workspace_snapshot_before.contains((*path).as_str()))
            .filter(|path| path_looks_like_source_file(path))
            .filter(|path| {
                mutation_tool_by_file
                    .get(*path)
                    .and_then(Value::as_array)
                    .is_some_and(|tools| {
                        let used_write = tools.iter().any(|value| value.as_str() == Some("write"));
                        let used_safe_patch = tools.iter().any(|value| {
                            matches!(value.as_str(), Some("edit") | Some("apply_patch"))
                        });
                        used_write && !used_safe_patch
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        if !unsafe_raw_write_paths.is_empty() {
            rejected_reason = Some(format!(
                "unsafe raw source rewrite rejected: {}",
                unsafe_raw_write_paths.join(", ")
            ));
        }
    }

    if let Some((path, text)) = accepted_output.clone() {
        let session_write_candidates = session_write_candidates_for_output(
            session,
            workspace_root,
            &path,
            run_id,
            runtime_values,
        );
        let requested_tools_for_contract = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let requested_has_read = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
        let requested_has_websearch = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| {
                tools
                    .iter()
                    .any(|value| value.as_str() == Some("websearch"))
            });
        let executed_has_mcp_list = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("mcp_list")));
        let current_executed_has_read = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| tools.iter().any(|value| value.as_str() == Some("read")));
        let canonical_read_paths = automation_attempt_evidence_read_paths(tool_telemetry);
        let upstream_has_read = use_upstream_evidence
            && upstream_evidence.is_some_and(|evidence| !evidence.read_paths.is_empty());
        let executed_has_read =
            current_executed_has_read || !canonical_read_paths.is_empty() || upstream_has_read;
        let latest_web_research_failure = tool_telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str);
        let canonical_web_research_status =
            automation_attempt_evidence_web_research_status(tool_telemetry);
        let web_research_backend_unavailable = canonical_web_research_status
            .as_deref()
            .is_some_and(|status| status == "unavailable")
            || web_research_unavailable(latest_web_research_failure);
        let web_research_unavailable = !requested_has_websearch || web_research_backend_unavailable;
        let web_research_expected =
            enforcement_requires_external_sources(&enforcement) && !web_research_unavailable;
        let current_web_research_succeeded = canonical_web_research_status
            .as_deref()
            .is_some_and(|status| status == "succeeded")
            || tool_telemetry
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        let web_research_succeeded = current_web_research_succeeded
            || (use_upstream_evidence
                && upstream_evidence.is_some_and(|evidence| evidence.web_research_succeeded));
        let connector_discovery_text = automation_connector_hint_text(node);
        let connector_discovery_required =
            tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
                &connector_discovery_text,
            );
        let selected_mcp_server_names = tool_telemetry
            .get("capability_resolution")
            .and_then(|value| value.get("mcp_tool_diagnostics"))
            .and_then(|value| value.get("selected_servers"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let connector_action_patterns =
            automation_requested_server_scoped_mcp_tools(node, &selected_mcp_server_names);
        let executed_concrete_mcp_tool = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| {
                tools.iter().filter_map(Value::as_str).any(|tool_name| {
                    tool_name != "mcp_list"
                        && connector_action_patterns.iter().any(|pattern| {
                            tandem_core::tool_name_matches_policy(pattern, tool_name)
                        })
                })
            });
        let workspace_inspection_satisfied = tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || executed_has_read
            || (use_upstream_evidence && !discovered_relevant_paths.is_empty());
        if connector_discovery_required
            && !executed_has_mcp_list
            && !enforcement::automation_node_prefers_mcp_servers(node)
        {
            unmet_requirements.push("mcp_discovery_missing".to_string());
        }
        if automation_node_is_outbound_action(node)
            && !automation_node_requires_email_delivery(node)
            && !connector_action_patterns.is_empty()
            && !executed_concrete_mcp_tool
        {
            unmet_requirements.push("mcp_connector_action_missing".to_string());
        }
        let prewrite_requirements =
            automation_node_prewrite_requirements(node, &requested_tools_for_contract);
        let session_text_recovery_requires_prewrite =
            enforcement.session_text_recovery.as_deref() == Some("require_prewrite_satisfied");
        let session_text_recovery_allowed =
            prewrite_requirements.as_ref().is_none_or(|requirements| {
                !session_text_recovery_requires_prewrite
                    || repair_exhausted_hint
                    || ((!requirements.workspace_inspection_required
                        || workspace_inspection_satisfied)
                        && (!requirements.concrete_read_required || executed_has_read)
                        && (!requirements.successful_web_research_required
                            || web_research_succeeded))
            });
        let upstream_read_paths = upstream_evidence
            .map(|evidence| evidence.read_paths.clone())
            .unwrap_or_default();
        let upstream_citations = upstream_evidence
            .map(|evidence| evidence.citations.clone())
            .unwrap_or_default();
        let mut candidate_assessments = session_write_candidates
            .iter()
            .map(|candidate| {
                assess_artifact_candidate(
                    node,
                    workspace_root,
                    "session_write",
                    candidate,
                    &read_paths,
                    &discovered_relevant_paths,
                    &upstream_read_paths,
                    &upstream_citations,
                )
            })
            .collect::<Vec<_>>();
        let executed_tools_for_attempt = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let required_output_path =
            automation_node_required_output_path_with_runtime_for_run(node, run_id, runtime_values);
        let current_attempt_output_materialized_via_filesystem =
            required_output_path.as_ref().is_some_and(|output_path| {
                session_write_materialized_output_for_output(
                    session,
                    workspace_root,
                    output_path,
                    run_id,
                    runtime_values,
                )
            });
        let current_attempt_has_recorded_activity = !executed_tools_for_attempt.is_empty()
            || !session_write_candidates.is_empty()
            || verified_output_materialized
            || (use_upstream_evidence && upstream_evidence.is_some());
        let preexisting_output_reuse_allowed =
            automation_node_allows_preexisting_output_reuse(node);
        let current_attempt_output_materialized =
            current_attempt_output_materialized_via_filesystem || verified_output_materialized;
        let must_write_file_statuses = must_write_files
            .iter()
            .map(|required_path| {
                let resolved = resolve_automation_output_path(workspace_root, required_path).ok();
                let exists = resolved
                    .as_ref()
                    .is_some_and(|path| path.exists() && path.is_file());
                let touched_by_current_attempt = session_write_touched_output_for_output(
                    session,
                    workspace_root,
                    required_path,
                    None,
                    runtime_values,
                );
                let materialized_by_current_attempt = session_write_materialized_output_for_output(
                    session,
                    workspace_root,
                    required_path,
                    None,
                    runtime_values,
                );
                json!({
                    "path": required_path,
                    "resolved_path": resolved.map(|path| path.to_string_lossy().to_string()),
                    "exists": exists,
                    "touched_by_current_attempt": touched_by_current_attempt,
                    "materialized_by_current_attempt": materialized_by_current_attempt,
                })
            })
            .collect::<Vec<_>>();
        validation_basis = json!({
            "authority": "filesystem_and_receipts",
            "quality_mode": quality_mode_resolution.effective.stable_key(),
            "requested_quality_mode": quality_mode_resolution
                .requested
                .map(|mode| mode.stable_key()),
            "legacy_quality_rollback_enabled": quality_mode_resolution.legacy_rollback_enabled,
            "current_attempt_output_materialized": current_attempt_output_materialized,
            "current_attempt_output_materialized_via_filesystem": current_attempt_output_materialized_via_filesystem,
            "verified_output_materialized": verified_output_materialized,
            "required_output_path": required_output_path,
        });
        if let Some(object) = validation_basis.as_object_mut() {
            object.insert(
                "session_write_candidate_count".to_string(),
                json!(session_write_candidates.len()),
            );
            object.insert(
                "session_write_touched_output".to_string(),
                json!(session_write_touched_output_for_output(
                    session,
                    workspace_root,
                    &path,
                    run_id,
                    runtime_values,
                )),
            );
            object.insert(
                "current_attempt_has_recorded_activity".to_string(),
                json!(current_attempt_has_recorded_activity),
            );
            object.insert(
                "current_attempt_has_read".to_string(),
                json!(current_executed_has_read || !canonical_read_paths.is_empty()),
            );
            object.insert(
                "current_attempt_has_web_research".to_string(),
                json!(current_web_research_succeeded),
            );
            object.insert(
                "workspace_inspection_satisfied".to_string(),
                json!(workspace_inspection_satisfied),
            );
            object.insert(
                "upstream_evidence_used".to_string(),
                json!(use_upstream_evidence && upstream_evidence.is_some()),
            );
            object.insert("must_write_files".to_string(), json!(must_write_files));
            object.insert(
                "explicit_input_files".to_string(),
                json!(explicit_input_files),
            );
            object.insert(
                "explicit_output_files".to_string(),
                json!(explicit_output_files),
            );
            object.insert(
                "must_write_file_statuses".to_string(),
                json!(must_write_file_statuses),
            );
        }
        if !must_write_files.is_empty()
            && !must_write_file_statuses.iter().all(|item| {
                item.get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
        {
            unmet_requirements.push("required_workspace_files_missing".to_string());
        }
        let missing_current_attempt_output_write = requires_current_attempt_output
            && !current_attempt_output_materialized
            && !preexisting_output_reuse_allowed;
        if !missing_current_attempt_output_write && !text.trim().is_empty() {
            candidate_assessments.push(assess_artifact_candidate(
                node,
                workspace_root,
                "verified_output",
                &text,
                &read_paths,
                &discovered_relevant_paths,
                &upstream_read_paths,
                &upstream_citations,
            ));
        }
        let allow_preexisting_candidate = if preexisting_output_reuse_allowed {
            true
        } else {
            !requires_current_attempt_output
                && !automation_node_is_strict_quality(node)
                && (!enforcement_requires_evidence || current_attempt_has_recorded_activity)
        };
        if allow_preexisting_candidate {
            if let Some(previous) = preexisting_output.filter(|value| !value.trim().is_empty()) {
                candidate_assessments.push(assess_artifact_candidate(
                    node,
                    workspace_root,
                    "preexisting_output",
                    previous,
                    &read_paths,
                    &discovered_relevant_paths,
                    &upstream_read_paths,
                    &upstream_citations,
                ));
            }
        }
        if missing_current_attempt_output_write {
            accepted_output = None;
            accepted_candidate_source = Some("current_attempt_missing_output_write".to_string());
            unmet_requirements.push("current_attempt_output_missing".to_string());
            rejected_reason = Some(format!(
                "required output `{}` was not created in the current attempt",
                path
            ));
        } else if !allow_preexisting_candidate {
            accepted_candidate_source = Some("current_attempt_missing_activity".to_string());
        }
        let best_candidate = best_artifact_candidate(&candidate_assessments);
        artifact_candidates = candidate_assessments
            .iter()
            .map(|candidate| {
                let accepted = best_candidate.as_ref().is_some_and(|best| {
                    best.source == candidate.source && best.text == candidate.text
                });
                artifact_candidate_summary(candidate, accepted)
            })
            .collect::<Vec<_>>();
        if let Some(best) = best_candidate.clone() {
            accepted_candidate_source = Some(best.source.clone());
            reviewed_paths_backed_by_read = best.reviewed_paths_backed_by_read.clone();
            citation_count = best.citation_count;
            web_sources_reviewed_present = best.web_sources_reviewed_present;
            heading_count = best.heading_count;
            paragraph_count = best.paragraph_count;
            if discovered_relevant_paths.is_empty() {
                discovered_relevant_paths = best.reviewed_paths.clone();
            }
            unreviewed_relevant_paths = best.unreviewed_relevant_paths.clone();
            let best_is_verified_output = best.source == "verified_output" && best.text == text;
            if !best_is_verified_output {
                if session_text_recovery_allowed {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, &path) {
                        let _ = std::fs::write(&resolved, &best.text);
                        accepted_output = Some((path.clone(), best.text.clone()));
                    }
                }
                recovered_from_session_write =
                    session_text_recovery_allowed && best.source == "session_write";
            } else {
                accepted_output = Some((path.clone(), best.text.clone()));
            }
        } else if missing_current_attempt_output_write {
            if rejected_reason.is_none() {
                rejected_reason = Some(format!(
                    "required output `{}` was not created in the current attempt",
                    path
                ));
            }
            semantic_block_reason =
                Some("required output was not created in the current attempt".to_string());
        }
        repair_attempted = session_write_candidates.len() > 1
            && (requested_has_read || web_research_expected)
            && (!reviewed_paths_backed_by_read.is_empty()
                || !read_paths.is_empty()
                || tool_telemetry
                    .get("tool_call_counts")
                    .and_then(|value| value.get("write"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 1);
        let selected_assessment = best_candidate.as_ref();
        let required_tools_for_node = enforcement.required_tools.clone();
        let has_required_tools = !required_tools_for_node.is_empty();
        let requires_local_source_reads = enforcement
            .required_evidence
            .iter()
            .any(|item| item == "local_source_reads");
        let requires_external_sources = enforcement
            .required_evidence
            .iter()
            .any(|item| item == "external_sources")
            && !web_research_unavailable;
        let requires_files_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "files_reviewed");
        let requires_files_not_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "files_not_reviewed");
        let requires_citations = enforcement
            .required_sections
            .iter()
            .any(|item| item == "citations");
        let requires_web_sources_reviewed = enforcement
            .required_sections
            .iter()
            .any(|item| item == "web_sources_reviewed")
            && !web_research_unavailable;
        let requires_local_source_reads =
            requires_local_source_reads && !mcp_grounded_citations_artifact;
        let requires_external_sources =
            requires_external_sources && !mcp_grounded_citations_artifact;
        let requires_files_reviewed = requires_files_reviewed && !mcp_grounded_citations_artifact;
        let requires_files_not_reviewed =
            requires_files_not_reviewed && !mcp_grounded_citations_artifact;
        let requires_citations = requires_citations && !mcp_grounded_citations_artifact;
        let requires_web_sources_reviewed =
            requires_web_sources_reviewed && !mcp_grounded_citations_artifact;
        let has_research_contract = requires_local_source_reads
            || requires_external_sources
            || requires_files_reviewed
            || requires_files_not_reviewed
            || requires_citations
            || requires_web_sources_reviewed;
        let optional_workspace_reads =
            enforcement::automation_node_allows_optional_workspace_reads(node);
        let requires_read = required_tools_for_node.iter().any(|tool| tool == "read");
        let requires_websearch = required_tools_for_node
            .iter()
            .any(|tool| tool == "websearch")
            && !web_research_unavailable;
        if has_research_contract && (requested_has_read || requires_local_source_reads) {
            let missing_concrete_reads =
                !optional_workspace_reads && requires_local_source_reads && !executed_has_read;
            let missing_named_source_reads = !missing_required_source_read_paths.is_empty();
            let files_reviewed_backed = selected_assessment.is_some_and(|assessment| {
                !assessment.reviewed_paths.is_empty()
                    && assessment.reviewed_paths.len()
                        == assessment.reviewed_paths_backed_by_read.len()
            });
            let missing_file_coverage = (requires_files_reviewed
                && !selected_assessment
                    .is_some_and(|assessment| assessment.files_reviewed_present))
                || (requires_files_reviewed && !files_reviewed_backed)
                || (requires_files_not_reviewed && !unreviewed_relevant_paths.is_empty());
            let missing_web_research = requires_external_sources && !web_research_succeeded;
            let upstream_has_citations =
                use_upstream_evidence && upstream_evidence.is_some_and(|e| e.citation_count > 0);
            let missing_citations = requires_citations
                && !selected_assessment.is_some_and(|assessment| assessment.citation_count > 0)
                && !upstream_has_citations;
            let upstream_web_sources_reviewed = use_upstream_evidence
                && upstream_evidence.is_some_and(|e| e.web_research_succeeded);
            let missing_web_sources_reviewed = requires_web_sources_reviewed
                && !selected_assessment
                    .is_some_and(|assessment| assessment.web_sources_reviewed_present)
                && !upstream_web_sources_reviewed;
            let preserve_current_attempt_output_missing = !current_attempt_output_materialized
                && unmet_requirements
                    .iter()
                    .any(|value| value == "current_attempt_output_missing");
            let had_read_only_source_mutation = unmet_requirements
                .iter()
                .any(|value| value == "read_only_source_mutations");
            unmet_requirements.clear();
            if had_read_only_source_mutation {
                unmet_requirements.push("read_only_source_mutations".to_string());
            }
            if preserve_current_attempt_output_missing {
                unmet_requirements.push("current_attempt_output_missing".to_string());
            }
            let path_hygiene_failure = selected_assessment.and_then(|assessment| {
                validate_path_array_hygiene(&assessment.reviewed_paths)
                    .or_else(|| validate_path_array_hygiene(&assessment.unreviewed_relevant_paths))
            });
            if path_hygiene_failure.is_some() {
                unmet_requirements.push("files_reviewed_contains_nonconcrete_paths".to_string());
            }
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_named_source_reads {
                unmet_requirements.push("required_source_paths_not_read".to_string());
            }
            if missing_citations {
                unmet_requirements.push("citations_missing".to_string());
            }
            if requires_files_reviewed
                && !selected_assessment.is_some_and(|assessment| assessment.files_reviewed_present)
            {
                unmet_requirements.push("files_reviewed_missing".to_string());
            }
            if requires_files_reviewed && !files_reviewed_backed {
                unmet_requirements.push("files_reviewed_not_backed_by_read".to_string());
            }
            let strict_unreviewed_check = use_upstream_evidence
                && upstream_evidence
                    .as_ref()
                    .is_some_and(|e| !e.discovered_relevant_paths.is_empty());
            if requires_files_not_reviewed
                && !unreviewed_relevant_paths.is_empty()
                && !strict_unreviewed_check
            {
                unmet_requirements.push("relevant_files_not_reviewed_or_skipped".to_string());
            }
            if missing_web_sources_reviewed {
                unmet_requirements.push("web_sources_reviewed_missing".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            let has_path_hygiene_failure = path_hygiene_failure.is_some();
            if missing_concrete_reads
                || missing_named_source_reads
                || missing_citations
                || missing_file_coverage
                || missing_web_sources_reviewed
                || missing_web_research
                || has_path_hygiene_failure
            {
                semantic_block_reason = Some(if has_path_hygiene_failure {
                    "research artifact contains non-concrete paths (wildcards or directory placeholders) in source audit"
                        .to_string()
                } else if missing_named_source_reads {
                    "research completed without reading the exact required source files".to_string()
                } else if missing_concrete_reads {
                    "research completed without concrete file reads or required source coverage"
                        .to_string()
                } else if missing_web_research {
                    "research completed without required current web research".to_string()
                } else if !unreviewed_relevant_paths.is_empty() {
                    "research completed without covering or explicitly skipping relevant discovered files".to_string()
                } else if missing_citations {
                    "research completed without citation-backed claims".to_string()
                } else if missing_web_sources_reviewed {
                    "research completed without a web sources reviewed section".to_string()
                } else {
                    "research completed without a source-backed files reviewed section".to_string()
                });
            }
        }
        if !has_research_contract && has_required_tools {
            let missing_concrete_reads =
                !optional_workspace_reads && requires_read && !executed_has_read;
            let missing_named_source_reads = !missing_required_source_read_paths.is_empty();
            let missing_web_research =
                requires_websearch && requires_external_sources && !web_research_succeeded;
            if missing_concrete_reads {
                unmet_requirements.push("no_concrete_reads".to_string());
            }
            if missing_named_source_reads {
                unmet_requirements.push("required_source_paths_not_read".to_string());
            }
            if missing_web_research {
                unmet_requirements.push("missing_successful_web_research".to_string());
            }
            if missing_concrete_reads || missing_named_source_reads || missing_web_research {
                semantic_block_reason = Some(if missing_named_source_reads {
                    "artifact finalized without reading the exact required source files".to_string()
                } else {
                    "artifact finalized without using required tools".to_string()
                });
            }
        }
        let strict_quality_mode = enforcement::automation_node_is_strict_quality(node);
        if strict_quality_mode
            && validator_kind == crate::AutomationOutputValidatorKind::GenericArtifact
        {
            let contract_kind = node
                .output_contract
                .as_ref()
                .map(|contract| contract.kind.trim().to_ascii_lowercase())
                .unwrap_or_default();
            let selected = selected_assessment.cloned();
            let upstream_citation_count = upstream_evidence
                .map(|evidence| evidence.citation_count)
                .unwrap_or(0);
            let upstream_read_count = upstream_evidence
                .map(|evidence| evidence.read_paths.len())
                .unwrap_or(0);
            let upstream_evidence_anchor_target =
                source_evidence_anchor_target(&upstream_read_paths, &upstream_citations);
            let upstream_web_research_succeeded = upstream_evidence
                .map(|evidence| evidence.web_research_succeeded)
                .unwrap_or(false);
            let requires_rich_upstream_synthesis =
                automation_node_uses_upstream_validation_evidence(node)
                    && matches!(contract_kind.as_str(), "report_markdown" | "text_summary");
            let requires_inline_source_sections = enforcement
                .required_sections
                .iter()
                .any(|section| matches!(section.as_str(), "citations" | "web_sources_reviewed"));
            let missing_editorial_substance =
                matches!(contract_kind.as_str(), "report_markdown" | "text_summary")
                    && !selected.as_ref().is_some_and(|assessment| {
                        !assessment.placeholder_like
                            && (assessment.substantive
                                || (assessment.length >= 120 && assessment.paragraph_count >= 1))
                    });
            let missing_markdown_structure = contract_kind == "report_markdown"
                && !selected.as_ref().is_some_and(|assessment| {
                    assessment.heading_count >= 1 && assessment.paragraph_count >= 2
                });
            let missing_upstream_synthesis = requires_rich_upstream_synthesis
                && (upstream_read_count > 0
                    || upstream_citation_count > 0
                    || upstream_web_research_succeeded)
                && !selected.as_ref().is_some_and(|assessment| {
                    !assessment.placeholder_like
                        && assessment.substantive
                        && assessment.length >= 600
                        && (assessment.heading_count >= 4
                            || (assessment.heading_count >= 2 && assessment.paragraph_count >= 2)
                            || (assessment.heading_count >= 2 && assessment.list_count >= 4))
                        && assessment.evidence_anchor_count >= upstream_evidence_anchor_target
                        && (!requires_inline_source_sections
                            || upstream_citation_count == 0
                            || assessment.citation_count >= 1
                            || assessment.web_sources_reviewed_present)
                });
            let bare_relative_artifact_href =
                matches!(contract_kind.as_str(), "report_markdown" | "text_summary")
                    && selected.as_ref().is_some_and(|assessment| {
                        contains_bare_tandem_artifact_href(&assessment.text)
                    });
            if missing_editorial_substance {
                unmet_requirements.push("editorial_substance_missing".to_string());
            }
            if missing_markdown_structure {
                unmet_requirements.push("markdown_structure_missing".to_string());
            }
            if missing_upstream_synthesis {
                unmet_requirements.push("upstream_evidence_not_synthesized".to_string());
            }
            if bare_relative_artifact_href {
                unmet_requirements.push("bare_relative_artifact_href".to_string());
            }
            if semantic_block_reason.is_none()
                && (missing_editorial_substance
                    || missing_markdown_structure
                    || missing_upstream_synthesis
                    || bare_relative_artifact_href)
            {
                semantic_block_reason = Some(if missing_upstream_synthesis {
                    "final artifact does not adequately synthesize the available upstream evidence"
                        .to_string()
                } else if missing_markdown_structure {
                    "editorial artifact is missing expected markdown structure".to_string()
                } else if bare_relative_artifact_href {
                    "final artifact contains a bare relative artifact href; use a canonical run-scoped link or plain text instead"
                        .to_string()
                } else {
                    "editorial artifact is too weak or placeholder-like".to_string()
                });
            }
        }
        let explicit_completed = parsed_status
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| value.eq_ignore_ascii_case("completed"));
        let writes_blocked_handoff_artifact = !explicit_completed
            && accepted_output
                .as_ref()
                .map(|(_, accepted_text)| accepted_text.to_ascii_lowercase())
                .is_some_and(|lowered| {
                    (lowered.contains("status: blocked")
                        || lowered.contains("blocked pending")
                        || lowered.contains("node produced a blocked handoff artifact"))
                        && (lowered.contains("cannot be finalized")
                            || lowered.contains("required source reads")
                            || lowered.contains("web research")
                            || lowered.contains("toolset available"))
                });
        if has_research_contract
            && semantic_block_reason.is_some()
            && writes_blocked_handoff_artifact
        {
            if let Some((path, _)) = accepted_output.as_ref() {
                if let Some(previous) = preexisting_output.filter(|value| !value.trim().is_empty())
                {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, path) {
                        let _ = std::fs::write(&resolved, previous);
                    }
                    accepted_output = None;
                    accepted_candidate_source = None;
                    blocked_handoff_cleanup_action =
                        Some("restored_preexisting_output".to_string());
                } else {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, path) {
                        let _ = std::fs::remove_file(&resolved);
                    }
                    accepted_output = None;
                    accepted_candidate_source = None;
                    blocked_handoff_cleanup_action = Some("removed_blocked_output".to_string());
                }
            }
        }
        let repair_promoted_after_write = repair_attempted
            && execution_mode == "artifact_write"
            && accepted_output.is_some()
            && session_write_touched_output_for_output(
                session,
                workspace_root,
                &path,
                run_id,
                runtime_values,
            );
        let repair_promoted_after_read_and_output_change = repair_attempted
            && execution_mode == "artifact_write"
            && accepted_output.is_some()
            && (current_executed_has_read || !canonical_read_paths.is_empty())
            && automation_repair_output_differs_from_preexisting(
                preexisting_output,
                accepted_output.as_ref(),
            );
        if !writes_blocked_handoff_artifact
            && (repair_promoted_after_write || repair_promoted_after_read_and_output_change)
        {
            semantic_block_reason = None;
            rejected_reason = None;
            let had_read_only_source_mutation = unmet_requirements
                .iter()
                .any(|value| value == "read_only_source_mutations");
            unmet_requirements.clear();
            if had_read_only_source_mutation {
                unmet_requirements.push("read_only_source_mutations".to_string());
            }
            repair_succeeded = true;
            if let Some(object) = validation_basis.as_object_mut() {
                object.insert(
                    "repair_promoted_after_write".to_string(),
                    json!(repair_promoted_after_write),
                );
                object.insert(
                    "repair_promoted_after_read_and_output_change".to_string(),
                    json!(repair_promoted_after_read_and_output_change),
                );
            }
        }
        if rejected_reason.is_none()
            && matches!(execution_mode, "git_patch" | "filesystem_patch")
            && preexisting_output.is_some()
            && path_looks_like_source_file(&path)
            && tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| {
                    tools.iter().any(|value| value.as_str() == Some("write"))
                        && !tools.iter().any(|value| value.as_str() == Some("edit"))
                        && !tools
                            .iter()
                            .any(|value| value.as_str() == Some("apply_patch"))
                })
        {
            rejected_reason =
                Some("code workflow used raw write without patch/edit safety".to_string());
        }
        if semantic_block_reason.is_some()
            && !recovered_from_session_write
            && selected_assessment.is_some_and(|assessment| !assessment.substantive)
        {
            // TODO(coding-hardening): Fold this recovery path into a single
            // artifact-finalization step that deterministically picks the best
            // candidate before node output is wrapped, instead of patching up the
            // final file after semantic validation fires.
            if let Some(best) = selected_assessment
                .filter(|assessment| assessment.substantive)
                .cloned()
            {
                if session_text_recovery_allowed {
                    if let Ok(resolved) = resolve_automation_output_path(workspace_root, &path) {
                        let _ = std::fs::write(&resolved, &best.text);
                        accepted_output = Some((path.clone(), best.text.clone()));
                        recovered_from_session_write = best.source == "session_write";
                        repair_succeeded = true;
                        accepted_candidate_source = Some(best.source.clone());
                    }
                }
            }
        }
        if repair_attempted && semantic_block_reason.is_none() {
            repair_succeeded = true;
        }
        if semantic_block_reason.is_some()
            && enforcement_requires_evidence
            && !current_attempt_has_recorded_activity
        {
            accepted_output = None;
        }
    }
    if accepted_output.is_some() && accepted_candidate_source.is_none() {
        accepted_candidate_source = Some("verified_output".to_string());
    }
    if handoff_only_structured_json {
        let requested_tools = tool_telemetry
            .get("requested_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let executed_tools = tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let requested_has_websearch = requested_tools
            .iter()
            .any(|value| value.as_str() == Some("websearch"));
        let executed_has_mcp_list = executed_tools
            .iter()
            .any(|value| value.as_str() == Some("mcp_list"));
        let executed_has_read = executed_tools
            .iter()
            .any(|value| value.as_str() == Some("read"));
        let latest_web_research_failure = tool_telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str);
        let web_research_unavailable =
            !requested_has_websearch || web_research_unavailable(latest_web_research_failure);
        let web_research_succeeded = tool_telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let workspace_inspection_satisfied = tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || executed_has_read
            || !current_discovered_relevant_paths.is_empty();
        let connector_discovery_text = [
            node.objective.as_str(),
            node.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("builder"))
                .and_then(Value::as_object)
                .and_then(|builder| builder.get("prompt"))
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ]
        .join("\n");
        let connector_discovery_required =
            tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
                &connector_discovery_text,
            );
        let requires_read = enforcement.required_tools.iter().any(|tool| tool == "read");
        let requires_websearch = enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "websearch")
            && !web_research_unavailable;
        let requires_workspace_inspection = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "workspace_inspection");
        let requires_concrete_reads = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "concrete_reads");
        let requires_successful_web_research = enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research")
            && !web_research_unavailable;
        let optional_workspace_reads =
            enforcement::automation_node_allows_optional_workspace_reads(node);

        if structured_handoff.is_none() {
            unmet_requirements.push("structured_handoff_missing".to_string());
        }
        if requires_workspace_inspection && !workspace_inspection_satisfied {
            unmet_requirements.push("workspace_inspection_required".to_string());
        }
        if !optional_workspace_reads
            && (requires_read || requires_concrete_reads)
            && !executed_has_read
        {
            unmet_requirements.push("no_concrete_reads".to_string());
        }
        if !missing_required_source_read_paths.is_empty() {
            unmet_requirements.push("required_source_paths_not_read".to_string());
        }
        if !optional_workspace_reads && requires_concrete_reads && !executed_has_read {
            unmet_requirements.push("concrete_read_required".to_string());
        }
        if (requires_websearch || requires_successful_web_research) && !web_research_succeeded {
            unmet_requirements.push("missing_successful_web_research".to_string());
        }
        if connector_discovery_required
            && !executed_has_mcp_list
            && !enforcement::automation_node_prefers_mcp_servers(node)
        {
            unmet_requirements.push("mcp_discovery_missing".to_string());
        }
        unmet_requirements.sort();
        unmet_requirements.dedup();
    }
    let validation_profile = enforcement
        .validation_profile
        .clone()
        .unwrap_or_else(|| "artifact_only".to_string());
    unmet_requirements.sort();
    unmet_requirements.dedup();
    let mut warning_requirements = unmet_requirements
        .iter()
        .filter(|item| validation_requirement_is_warning(&validation_profile, item))
        .cloned()
        .collect::<Vec<_>>();
    unmet_requirements.retain(|item| !validation_requirement_is_warning(&validation_profile, item));
    warning_requirements.sort();
    warning_requirements.dedup();
    semantic_block_reason = semantic_block_reason_for_requirements(&unmet_requirements);
    if should_downgrade_auto_cleaned_marker_rejection(
        rejected_reason.as_deref(),
        auto_cleaned,
        semantic_block_reason.as_deref(),
        accepted_output.is_some(),
    ) {
        rejected_reason = None;
        warning_requirements.push("undeclared_marker_files_cleaned".to_string());
        warning_requirements.sort();
        warning_requirements.dedup();
    }
    let required_output_path_for_retry =
        automation_node_required_output_path_with_runtime_for_run(node, run_id, runtime_values);
    let current_attempt_output_materialized_for_retry = required_output_path_for_retry
        .as_ref()
        .is_some_and(|output_path| {
            session_write_materialized_output_for_output(
                session,
                workspace_root,
                output_path,
                run_id,
                runtime_values,
            ) || verified_output_materialized
        });
    if accepted_output.is_none()
        && requires_current_attempt_output
        && !current_attempt_output_materialized_for_retry
        && !automation_node_allows_preexisting_output_reuse(node)
    {
        if rejected_reason.is_none() {
            let missing_output_path = required_output_path_for_retry
                .clone()
                .unwrap_or_else(|| automation_node_required_output_path(node).unwrap_or_default());
            rejected_reason = Some(format!(
                "required output `{}` was not created in the current attempt",
                missing_output_path
            ));
        }
        if !unmet_requirements
            .iter()
            .any(|value| value == "current_attempt_output_missing")
        {
            unmet_requirements.push("current_attempt_output_missing".to_string());
        }
        if use_upstream_evidence
            && upstream_evidence.is_some_and(|evidence| {
                !evidence.read_paths.is_empty() || evidence.citation_count > 0
            })
            && !unmet_requirements
                .iter()
                .any(|value| value == "upstream_evidence_not_synthesized")
        {
            unmet_requirements.push("upstream_evidence_not_synthesized".to_string());
        }
        if semantic_block_reason.is_none() {
            semantic_block_reason =
                Some("required output was not created in the current attempt".to_string());
        }
    }
    let (repair_attempt, repair_attempts_remaining, repair_exhausted) = infer_artifact_repair_state(
        parsed_status.as_ref(),
        repair_attempted,
        repair_succeeded,
        semantic_block_reason.as_deref(),
        tool_telemetry,
    );
    let has_required_tools = !enforcement.required_tools.is_empty();
    let contract_requires_repair = !enforcement.retry_on_missing.is_empty()
        || has_required_tools
        || handoff_only_structured_json;
    let validation_outcome = if contract_requires_repair && semantic_block_reason.is_some() {
        if repair_exhausted {
            "blocked"
        } else {
            "needs_repair"
        }
    } else if semantic_block_reason.is_some() {
        "blocked"
    } else if !warning_requirements.is_empty() {
        "accepted_with_warnings"
    } else {
        "passed"
    };
    let should_classify = contract_requires_repair;
    let latest_web_research_failure = tool_telemetry
        .get("latest_web_research_failure")
        .and_then(Value::as_str);
    let requested_has_websearch = tool_telemetry
        .get("requested_tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools
                .iter()
                .any(|value| value.as_str() == Some("websearch"))
        });
    let web_research_expected_for_classification =
        enforcement_requires_external_sources(&enforcement)
            && requested_has_websearch
            && !web_research_unavailable(latest_web_research_failure);
    let external_research_mode = if enforcement_requires_external_sources(&enforcement) {
        if !requested_has_websearch || web_research_unavailable(latest_web_research_failure) {
            "waived_unavailable"
        } else {
            "required"
        }
    } else {
        "not_required"
    };
    let blocking_classification = if should_classify {
        classify_research_validation_state(
            &tool_telemetry
                .get("requested_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            &tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            web_research_expected_for_classification,
            &unmet_requirements,
            latest_web_research_failure,
            repair_exhausted,
        )
        .map(str::to_string)
    } else {
        None
    };
    let required_next_tool_actions = if should_classify {
        research_required_next_tool_actions(
            &tool_telemetry
                .get("requested_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            &tool_telemetry
                .get("executed_tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            web_research_expected_for_classification,
            &unmet_requirements,
            &missing_required_source_read_paths,
            &upstream_evidence
                .map(|e| e.read_paths.clone())
                .unwrap_or_default(),
            &upstream_evidence
                .map(|e| e.citations.clone())
                .unwrap_or_default(),
            &unreviewed_relevant_paths,
            latest_web_research_failure,
        )
    } else {
        Vec::new()
    };

    let metadata = json!({
        "accepted_artifact_path": accepted_output.as_ref().map(|(path, _)| path.clone()),
        "accepted_candidate_source": accepted_candidate_source,
        "rejected_artifact_reason": rejected_reason,
        "semantic_block_reason": semantic_block_reason,
        "recovered_from_session_write": recovered_from_session_write,
        "undeclared_files_created": undeclared_files_created,
        "auto_cleaned": auto_cleaned,
        "execution_policy": execution_policy,
        "touched_files": touched_files,
        "mutation_tool_by_file": Value::Object(mutation_tool_by_file),
        "read_only_source_mutation_events": Value::Array(read_only_source_mutations.clone()),
        "read_only_source_mutation_count": read_only_source_mutations.len(),
        "verification": verification_summary,
        "git_diff_summary": git_diff_summary_for_paths(workspace_root, &touched_files),
        "read_paths": read_paths,
        "upstream_read_paths": if use_upstream_evidence {
            json!(upstream_evidence.map_or(&[] as &[_], |e| e.read_paths.as_slice()))
        } else {
            json!([])
        },
        "current_node_read_paths": current_read_paths,
        "discovered_relevant_paths": discovered_relevant_paths,
        "current_node_discovered_relevant_paths": current_discovered_relevant_paths,
        "reviewed_paths_backed_by_read": reviewed_paths_backed_by_read,
        "unreviewed_relevant_paths": unreviewed_relevant_paths,
        "citation_count": if use_upstream_evidence {
            json!(citation_count.saturating_add(
                upstream_evidence.map(|e| e.citation_count).unwrap_or(0)
            ))
        } else {
            json!(citation_count)
        },
        "upstream_citations": if use_upstream_evidence {
            json!(upstream_evidence.map_or(&[] as &[_], |e| e.citations.as_slice()))
        } else {
            json!([])
        },
        "web_sources_reviewed_present": web_sources_reviewed_present,
        "heading_count": heading_count,
        "paragraph_count": paragraph_count,
        "web_research_attempted": if use_upstream_evidence {
            json!(tool_telemetry.get("web_research_used").and_then(Value::as_bool).unwrap_or(false)
                || upstream_evidence.is_some_and(|evidence| evidence.web_research_attempted))
        } else {
            tool_telemetry.get("web_research_used").cloned().unwrap_or(json!(false))
        },
        "web_research_succeeded": if use_upstream_evidence {
            json!(tool_telemetry.get("web_research_succeeded").and_then(Value::as_bool).unwrap_or(false)
                || upstream_evidence.is_some_and(|evidence| evidence.web_research_succeeded))
        } else {
            tool_telemetry.get("web_research_succeeded").cloned().unwrap_or(json!(false))
        },
        "external_research_mode": external_research_mode,
        "upstream_evidence_applied": use_upstream_evidence,
        "blocked_handoff_cleanup_action": blocked_handoff_cleanup_action,
        "repair_attempted": repair_attempted,
        "repair_attempt": repair_attempt,
        "repair_attempts_remaining": repair_attempts_remaining,
        "repair_budget_spent": repair_attempt > 0,
        "repair_succeeded": repair_succeeded,
        "repair_exhausted": repair_exhausted,
        "validation_outcome": validation_outcome,
        "validation_profile": validation_profile,
        "validation_basis": validation_basis,
        "blocking_classification": blocking_classification,
        "required_next_tool_actions": required_next_tool_actions,
        "unmet_requirements": unmet_requirements,
        "warning_requirements": warning_requirements.clone(),
        "warning_count": warning_requirements.len(),
        "artifact_candidates": artifact_candidates,
        "resolved_enforcement": enforcement,
        "structured_handoff_present": structured_handoff.is_some(),
    });
    let rejected = metadata
        .get("rejected_artifact_reason")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            metadata
                .get("semantic_block_reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    (accepted_output, metadata, rejected)
}
