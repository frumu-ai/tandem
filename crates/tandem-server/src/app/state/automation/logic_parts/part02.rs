fn repair_brief_strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    row.as_str()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                        .or_else(|| {
                            if row.is_object() || row.is_array() {
                                Some(truncate_text(&row.to_string(), 500))
                            } else {
                                None
                            }
                        })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn repair_brief_value(value: Option<&Value>) -> String {
    value
        .map(|value| truncate_text(&value.to_string(), 1200))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "not recorded".to_string())
}

fn repair_brief_review_strings(verdict: &Value, key: &str) -> Vec<String> {
    repair_brief_strings(verdict.pointer(&format!("/attempt_review/{key}")))
}

fn repair_brief_review_section(rows: &[String], fallback: &str) -> String {
    if rows.is_empty() {
        fallback.to_string()
    } else {
        rows.join("\n")
    }
}

fn repair_brief_remote_paths_from_strings(rows: &[String]) -> Vec<String> {
    let mut paths = Vec::new();
    for row in rows {
        for token in row.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';') {
            let path = token.trim_matches(|ch: char| {
                matches!(ch, '`' | '"' | '\'' | '[' | ']' | '(' | ')' | '.')
            });
            if path.starts_with("/mnt/files/") && !paths.iter().any(|seen| seen == path) {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

fn repair_brief_connector_remote_corrective_line(
    actions: &[String],
    unmet: &[String],
    explicit_paths: &[String],
) -> String {
    let mut paths = explicit_paths.to_vec();
    for path in repair_brief_remote_paths_from_strings(actions) {
        if !paths.iter().any(|seen| seen == &path) {
            paths.push(path);
        }
    }
    let connector_remote_missing = unmet
        .iter()
        .any(|value| value == "connector_remote_result_not_materialized");
    if paths.is_empty() && !connector_remote_missing {
        return String::new();
    }
    let path_line = if paths.is_empty() {
        "the exact `/mnt/files/...` path recorded in the connector capture artifact".to_string()
    } else {
        paths
            .iter()
            .map(|path| format!("`{path}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "\n\nCORRECTIVE — connector remote result must be materialized:\n- The connector returned a full result file at {}.\n- For this retry, call the available remote bash/workbench helper with code or command that directly opens that exact path before writing the run artifact.\n- Do not use `data_preview`, compacted chat previews, stale sandbox artifacts, or a newly invented `.tandem` path as the source.\n- The final artifact must be built from the opened remote result file and must satisfy the output schema.",
        path_line
    )
}

fn repair_brief_context_section(repair_context: Option<&Value>) -> String {
    let Some(context) = repair_context else {
        return String::new();
    };
    let failure_identity = context
        .get("failure_identity")
        .and_then(Value::as_str)
        .unwrap_or("not recorded");
    let lifecycle_status = context
        .get("lifecycle_status")
        .and_then(Value::as_str)
        .unwrap_or("not recorded");
    let preserve = repair_brief_strings(context.get("preserve"));
    let missing_evidence = repair_brief_strings(context.get("missing_evidence"));
    let smallest_repair = context
        .get("smallest_repair")
        .and_then(Value::as_str)
        .unwrap_or("Repair the smallest unmet contract item, then let validation prove success.");
    let success_condition = context
        .get("success_condition")
        .and_then(Value::as_str)
        .unwrap_or("Validation passes.");
    let reward_signal = repair_brief_value(context.get("reward_signal"));
    format!(
        "\n\nEvidence-backed Repair Context:\n- Failure identity: `{}`.\n- Lifecycle status: `{}`.\n- Preserve: {}.\n- Missing evidence: {}.\n- Smallest valid repair: {}.\n- Success condition: {}.\n- Positive progress signal after validation: {}.",
        failure_identity,
        lifecycle_status,
        if preserve.is_empty() {
            "none recorded".to_string()
        } else {
            preserve.join(" | ")
        },
        if missing_evidence.is_empty() {
            "none recorded".to_string()
        } else {
            missing_evidence.join(" | ")
        },
        smallest_repair,
        success_condition,
        reward_signal,
    )
}

fn render_automation_repair_brief_from_verdict(
    node: &AutomationFlowNode,
    verdict: &Value,
    repair_context: Option<&Value>,
    attempt: u32,
    max_attempts: u32,
    run_id: Option<&str>,
) -> String {
    let failure_class = verdict
        .get("failure_class")
        .and_then(Value::as_str)
        .unwrap_or("contract_miss");
    let validation_reason = verdict
        .get("validation_reason")
        .and_then(Value::as_str)
        .unwrap_or("the previous attempt did not satisfy the runtime contract");
    let unmet = repair_brief_strings(verdict.get("unmet_requirements"));
    let actions = repair_brief_strings(verdict.get("required_next_actions"));
    let connector_remote_corrective_line =
        repair_brief_connector_remote_corrective_line(&actions, &unmet, &[]);
    let final_attempt_line = if attempt >= max_attempts {
        let output_path = automation_node_required_output_path_for_run(node, run_id)
            .unwrap_or_else(|| "the declared output path".to_string());
        format!(
            "\n\nFinal attempt:\n- This is the last retry.\n- Write the complete artifact to `{}` before ending.\n- End with a terminal completed status when the best available deliverable has been written.",
            output_path
        )
    } else {
        String::new()
    };
    let review = verdict.get("attempt_review");
    let progress_label = review
        .and_then(|value| value.get("progress_label"))
        .and_then(Value::as_str)
        .unwrap_or("not recorded");
    let progress_score = review
        .and_then(|value| value.get("progress_score"))
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "not recorded".to_string());
    let completed_correctly = repair_brief_review_strings(verdict, "completed_correctly");
    let still_needed = repair_brief_review_strings(verdict, "still_needed");
    let why_it_matters = repair_brief_review_strings(verdict, "why_it_matters");
    let next_moves = repair_brief_review_strings(verdict, "next_moves");
    let context_section = repair_brief_context_section(repair_context);
    format!(
        "Repair Brief:\n- Node `{}` is being retried because the prior attempt did not yet satisfy governance validation.\n- Failure class: `{}`.\n- Reason: {}.\n\nAttempt Review:\n- Progress: {} ({}/100)\n\nWhat went well:\n{}\n\nStill needed:\n{}\n\nWhy this matters:\n{}\n\nNext move:\n{}\n\nExpected:\n{}\n\nObserved:\n{}\n\nRepair:\n- Satisfy the expected contract before finalizing the artifact.\n- Do not stop after discovery, summaries, or partial tool output.\n- If a connector/tool is unavailable or returns no usable data, record the exact limitation in the artifact and still write the required output when the contract allows it.{}{}\n\nUnmet requirements:\n{}\n\nRequired next actions:\n{}",
        node.node_id,
        failure_class,
        validation_reason,
        progress_label,
        progress_score,
        repair_brief_review_section(
            &completed_correctly,
            "No verified progress was recorded for the prior attempt."
        ),
        repair_brief_review_section(
            &still_needed,
            "Use the Expected and Observed sections below to identify the missing contract work."
        ),
        repair_brief_review_section(
            &why_it_matters,
            "Clear contract evidence lets the runtime repair safely instead of guessing."
        ),
        repair_brief_review_section(
            &next_moves,
            "Use the Expected and Observed sections below to repair the contract miss."
        ),
        repair_brief_value(verdict.get("expected")),
        repair_brief_value(verdict.get("observed")),
        final_attempt_line,
        connector_remote_corrective_line,
        if unmet.is_empty() {
            "none recorded".to_string()
        } else {
            unmet.join("\n")
        },
        if actions.is_empty() {
            "Use the Expected and Observed sections above to repair the contract miss.".to_string()
        } else {
            actions.join("\n")
        },
    ) + &context_section
}

pub(crate) fn render_automation_repair_brief(
    node: &AutomationFlowNode,
    prior_output: Option<&Value>,
    attempt: u32,
    max_attempts: u32,
    run_id: Option<&str>,
) -> Option<String> {
    if attempt <= 1 {
        return None;
    }
    let prior_output = prior_output?;
    if !automation_output_needs_repair(prior_output) {
        return None;
    }

    let validator_summary = prior_output.get("validator_summary");
    let artifact_validation = prior_output.get("artifact_validation");
    let tool_telemetry = prior_output
        .get("tool_telemetry")
        .cloned()
        .map(|mut value| {
            automation_reset_attempt_tool_failure_labels(&mut value);
            value
        });
    let validator_outcome = validator_summary
        .and_then(|value| value.get("outcome"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let unmet_requirements_from_summary = validator_summary
        .and_then(|value| value.get("unmet_requirements"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let is_upstream_passed = validator_outcome
        .is_some_and(|outcome| outcome.eq_ignore_ascii_case("passed"))
        && unmet_requirements_from_summary.is_empty();
    if is_upstream_passed {
        return None;
    }
    if let Some(verdict) = prior_output.get("attempt_verdict") {
        return Some(render_automation_repair_brief_from_verdict(
            node,
            verdict,
            prior_output.get("repair_context"),
            attempt,
            max_attempts,
            run_id,
        ));
    }
    let reason = validator_summary
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            artifact_validation
                .and_then(|value| value.get("semantic_block_reason"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("the previous attempt did not satisfy the runtime validator");
    let unmet_requirements = unmet_requirements_from_summary;
    let mut blocking_classification = artifact_validation
        .and_then(|value| value.get("blocking_classification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unspecified")
        .to_string();
    let mut required_next_tool_actions = artifact_validation
        .and_then(|value| value.get("required_next_tool_actions"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if unmet_requirements
        .iter()
        .any(|value| value == "upstream_evidence_not_synthesized")
        && !required_next_tool_actions.iter().any(|value| {
            value.contains("Read and synthesize the strongest upstream artifacts before finalizing")
        })
    {
        required_next_tool_actions.push(
            "Read and synthesize the strongest upstream artifacts before finalizing.".to_string(),
        );
    }
    let validation_basis = artifact_validation
        .and_then(|value| value.get("validation_basis"))
        .and_then(Value::as_object);
    let current_attempt_has_recorded_activity = validation_basis
        .and_then(|basis| basis.get("current_attempt_has_recorded_activity"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let upstream_read_paths = validation_basis
        .and_then(|basis| basis.get("upstream_read_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let required_source_read_paths = validation_basis
        .and_then(|basis| basis.get("required_source_read_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let missing_required_source_read_paths = validation_basis
        .and_then(|basis| basis.get("missing_required_source_read_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let validation_basis_line = validation_basis
        .map(|basis| {
            let authority = basis
                .get("authority")
                .and_then(Value::as_str)
                .unwrap_or("unspecified");
            let current_attempt_output_materialized = basis
                .get("current_attempt_output_materialized")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_recorded_activity = basis
                .get("current_attempt_has_recorded_activity")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_read = basis
                .get("current_attempt_has_read")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let current_attempt_has_web_research = basis
                .get("current_attempt_has_web_research")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let workspace_inspection_satisfied = basis
                .get("workspace_inspection_satisfied")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            format!(
                "authority={}, output_materialized={}, recorded_activity={}, read={}, web_research={}, workspace_inspection={}",
                authority,
                current_attempt_output_materialized,
                current_attempt_has_recorded_activity,
                current_attempt_has_read,
                current_attempt_has_web_research,
                workspace_inspection_satisfied
            )
        })
        .unwrap_or_else(|| "none recorded".to_string());
    let required_source_read_paths_line = if required_source_read_paths.is_empty() {
        "none recorded".to_string()
    } else {
        required_source_read_paths.join(", ")
    };
    let missing_required_source_read_paths_line = if missing_required_source_read_paths.is_empty() {
        "none recorded".to_string()
    } else {
        missing_required_source_read_paths.join(", ")
    };
    let upstream_read_paths_line = if upstream_read_paths.is_empty() {
        "none recorded".to_string()
    } else {
        upstream_read_paths.join(", ")
    };
    if blocking_classification == "execution_error" && current_attempt_has_recorded_activity {
        blocking_classification = "artifact_write_missing".to_string();
    }
    if current_attempt_has_recorded_activity
        && required_next_tool_actions.iter().any(|action| {
            action
                .to_ascii_lowercase()
                .contains("retry after provider connectivity recovers")
        })
    {
        required_next_tool_actions =
            vec!["write the required run artifact to the declared output path".to_string()];
    }
    let tools_offered = tool_telemetry
        .as_ref()
        .and_then(|value| value.get("requested_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tools_executed = tool_telemetry
        .as_ref()
        .and_then(|value| value.get("executed_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unreviewed_relevant_paths = artifact_validation
        .and_then(|value| value.get("unreviewed_relevant_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let repair_attempt = artifact_validation
        .and_then(|value| value.get("repair_attempt"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(attempt.saturating_sub(1));
    let repair_attempts_remaining = artifact_validation
        .and_then(|value| value.get("repair_attempts_remaining"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_else(|| max_attempts.saturating_sub(attempt.saturating_sub(1)));

    let unmet_line = if unmet_requirements.is_empty() {
        "none recorded".to_string()
    } else {
        unmet_requirements.join(", ")
    };
    let tools_offered_line = if tools_offered.is_empty() {
        if current_attempt_has_recorded_activity {
            "not recorded (but session activity was detected)".to_string()
        } else {
            "none recorded".to_string()
        }
    } else {
        tools_offered.join(", ")
    };
    let tools_executed_line = if tools_executed.is_empty() {
        "none recorded".to_string()
    } else {
        tools_executed.join(", ")
    };
    let unreviewed_line = if unreviewed_relevant_paths.is_empty() {
        "none recorded".to_string()
    } else {
        unreviewed_relevant_paths.join(", ")
    };
    let next_actions_line = if required_next_tool_actions.is_empty() {
        "none recorded".to_string()
    } else {
        required_next_tool_actions.join(" | ")
    };
    let mut connector_remote_file_paths = artifact_validation
        .and_then(|value| value.get("connector_remote_file_paths"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if connector_remote_file_paths.is_empty() {
        connector_remote_file_paths = artifact_validation
            .and_then(|value| value.get("connector_capture"))
            .and_then(|value| value.get("remote_file_paths"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
    }
    let connector_remote_corrective_line = repair_brief_connector_remote_corrective_line(
        &required_next_tool_actions,
        &unmet_requirements,
        &connector_remote_file_paths,
    );
    let code_workflow_line = if automation_node_is_code_workflow(node) {
        let verification_command =
            automation_node_verification_command(node).unwrap_or_else(|| {
                "run the most relevant repo-local build, test, or lint commands".to_string()
            });
        let write_scope =
            automation_node_write_scope(node).unwrap_or_else(|| "repo-scoped edits".to_string());
        format!(
            "\n- Code workflow repair path: inspect the touched files in `{}` first, patch with `edit` or `apply_patch` before any new `write`, then rerun verification with `{}` and fix the smallest failing root cause.",
            write_scope,
            verification_command
        )
    } else {
        String::new()
    };
    let final_attempt_line = if repair_attempts_remaining <= 1 {
        let output_path = automation_node_required_output_path_for_run(node, run_id)
            .unwrap_or_else(|| "the declared output path".to_string());
        format!(
            "\n\nFINAL ATTEMPT:\n- This is the last retry.\n- The engine will validate the output file at `{}` when this attempt ends.\n- Do not ask follow-up questions.\n- Do not end with a summary.\n- Write the complete artifact to the output path and include {{\"status\":\"completed\"}} as the last line of your response.",
            output_path
        )
    } else {
        String::new()
    };

    // Detect the "declared-output mistaken for input" failure mode: the prior
    // attempt claimed a required source file was missing, but the filename is
    // actually a declared OUTPUT for this node. Inject a corrective note so
    // the next attempt treats the path as a write target instead of reading
    // it and blocking on ENOENT.
    let declared_artifacts =
        super::prompting_impl::automation_node_declared_artifacts_to_create(node, None);
    let misread_artifacts: Vec<String> = if declared_artifacts.is_empty() {
        Vec::new()
    } else {
        let prior_summary = prior_output
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("");
        let prior_blocked_reason = prior_output
            .get("blocked_reason")
            .and_then(Value::as_str)
            .unwrap_or("");
        let haystack = format!("{} {}", prior_summary, prior_blocked_reason).to_ascii_lowercase();
        let mentions_missing = haystack.contains("missing")
            || haystack.contains("not present")
            || haystack.contains("enoent")
            || haystack.contains("no such file")
            || haystack.contains("does not exist")
            || haystack.contains("not found");
        if !mentions_missing {
            Vec::new()
        } else {
            declared_artifacts
                .iter()
                .filter(|path| {
                    let lowered_path = path.to_ascii_lowercase();
                    let filename = std::path::Path::new(path)
                        .file_name()
                        .and_then(|v| v.to_str())
                        .map(|v| v.to_ascii_lowercase());
                    haystack.contains(&lowered_path)
                        || filename.is_some_and(|name| haystack.contains(&name))
                })
                .cloned()
                .collect()
        }
    };
    let declared_output_corrective_line = if misread_artifacts.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nCORRECTIVE — declared outputs were misread as inputs:\n- The previous attempt blocked claiming these files were missing as sources: {}.\n- These paths are DECLARED OUTPUTS for THIS node to CREATE. They do NOT exist as prerequisite inputs and were never expected to.\n- For this retry: do NOT call `read` on them. Use `write`, `edit`, or `apply_patch` to create them with their full content. ENOENT on these paths is expected; proceed with `write` anyway.\n- Do NOT return a blocked status because these paths were absent — create them.",
            misread_artifacts
                .iter()
                .map(|path| format!("`{}`", path))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let nonterminal_status_corrective_line = if unmet_requirements
        .iter()
        .any(|value| value == "artifact_status_not_terminal")
    {
        "\n\nCORRECTIVE — artifact status must be terminal:\n- The previous artifact used a non-terminal status such as `blocked`, `needs_repair`, `incomplete`, or `in_progress`, so the engine rejected it.\n- For this retry, rewrite the complete artifact with top-level `status: \"completed\"` when you have produced the best available deliverable.\n- Record unavailable connectors, missing evidence, or source caveats under `limitations`, `source_limitations`, or `connector_limitations`; do not encode those limitations as the artifact status.".to_string()
    } else {
        String::new()
    };
    let requires_mcp_source_corrective =
        !enforcement::automation_node_allows_optional_connector_references(node)
            && unmet_requirements.iter().any(|value| {
                value == "mcp_connector_source_missing"
                    || value == "mcp_connector_source_artifact_missing"
            });
    let concrete_mcp_corrective_line = if requires_mcp_source_corrective {
        let concrete_tools =
            super::prompting_impl::automation_node_concrete_mcp_tool_allowlist(node);
        if concrete_tools.is_empty() {
            "\n\nCORRECTIVE — connector source evidence is required:\n- The previous attempt only proved connector discovery/inventory, not source inspection.\n- For this retry, call a concrete `mcp.*` source tool after `mcp_list` and before writing the artifact.\n- If the concrete connector call fails or returns no useful results, record that exact tool failure or empty result under `connector_limitations`, then write a completed artifact.".to_string()
        } else {
            format!(
                "\n\nCORRECTIVE — connector source evidence is required:\n- The previous attempt only proved connector discovery/inventory, not source inspection.\n- `mcp_list` alone is not enough. For this retry, call at least one concrete source tool after `mcp_list` and before writing the artifact: {}.\n- If the concrete connector call fails or returns no useful results, record that exact tool failure or empty result under `connector_limitations`, then write a completed artifact.",
                concrete_tools
                    .iter()
                    .map(|tool| format!("`{}`", tool))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    } else {
        String::new()
    };
    let required_source_read_corrective_line = if missing_required_source_read_paths.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nCORRECTIVE — exact source files are mandatory:\n- The previous attempt finalized without reading required source-of-truth file(s): {}.\n- For this retry, the first source action must be `read` on each exact missing path before `websearch`, `write`, `edit`, or `apply_patch`.\n- Do not use `glob`, `grep`, `codesearch`, summaries, or similarly named files as substitutes for these exact reads.\n- Do not finalize the artifact until these exact paths have been read in this attempt.",
            missing_required_source_read_paths
                .iter()
                .map(|path| format!("`{}`", path))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let web_research_receipt_corrective_line = if unmet_requirements
        .iter()
        .any(|value| value == "web_research_artifact_contradicts_tool_receipts")
    {
        "\n\nCORRECTIVE — web research receipts override artifact prose:\n- The previous attempt successfully executed web research, but the artifact claimed web research was unavailable.\n- For this retry, do not write `web_research.status: unavailable`, `unavailable_in_current_tooling`, or similar no-tool/no-source language.\n- Use the URLs and result summaries from the prior websearch/webfetch tool output, or call websearch again if you need fresher details, then write a citation-backed completed artifact.".to_string()
    } else {
        String::new()
    };

    Some(format!(
        "Repair Brief:\n- Node `{}` is being retried because the previous attempt ended in `needs_repair`.\n- Previous validation reason: {}.\n- Validation basis: {}.\n- Upstream read paths available for synthesis: {}.\n- Required source read paths: {}.\n- Missing required source read paths: {}.\n- Unmet requirements: {}.\n- Blocking classification: {}.\n- Required next tool actions: {}.\n- Tools offered last attempt: {}.\n- Tools executed last attempt: {}.\n- Relevant files still unread or explicitly unreviewed: {}.\n- Previous repair attempt count: {}.\n- Remaining repair attempts after this run: {}{}.\n- For this retry, satisfy the unmet requirements before finalizing the artifact.\n- Do not write a blocked handoff unless the required tools were actually attempted and remained unavailable or failed.{}{}{}{}{}{}{}",
        node.node_id,
        reason,
        validation_basis_line,
        upstream_read_paths_line,
        required_source_read_paths_line,
        missing_required_source_read_paths_line,
        unmet_line,
        blocking_classification,
        next_actions_line,
        tools_offered_line,
        tools_executed_line,
        unreviewed_line,
        repair_attempt,
        repair_attempts_remaining.saturating_sub(1),
        code_workflow_line,
        final_attempt_line,
        declared_output_corrective_line,
        nonterminal_status_corrective_line,
        concrete_mcp_corrective_line,
        required_source_read_corrective_line,
        web_research_receipt_corrective_line,
        connector_remote_corrective_line,
    ))
}

pub(crate) fn automation_concrete_mcp_repair_tool_allowlist(
    node: &AutomationFlowNode,
    prior_output: Option<&Value>,
) -> Vec<String> {
    let Some(prior_output) = prior_output else {
        return Vec::new();
    };
    if !automation_output_needs_repair(prior_output) {
        return Vec::new();
    }
    let has_connector_source_miss = prior_output
        .get("repair_context")
        .and_then(|value| value.get("unmet_requirements"))
        .or_else(|| prior_output.pointer("/attempt_verdict/unmet_requirements"))
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().filter_map(Value::as_str).any(|value| {
                matches!(
                    value,
                    "mcp_connector_source_missing"
                        | "mcp_connector_source_artifact_missing"
                        | "mcp_required_tool_missing"
                )
            })
        });
    if !has_connector_source_miss {
        return Vec::new();
    }
    let mut tools = prior_output
        .pointer("/repair_context/expected_contract/concrete_mcp_tools")
        .or_else(|| prior_output.pointer("/attempt_verdict/expected/concrete_mcp_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|tool| {
                    tool.starts_with("mcp.") && !tool.ends_with(".*") && *tool != "mcp_list"
                })
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if tools.is_empty() {
        tools = super::prompting_impl::automation_node_concrete_mcp_tool_allowlist(node);
    }
    if !tools.is_empty() && automation_node_required_output_path(node).is_some() {
        tools.push("write".to_string());
    }
    tools.sort();
    tools.dedup();
    tools
}

pub(crate) fn is_agent_standup_automation(automation: &AutomationV2Spec) -> bool {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("feature"))
        .and_then(Value::as_str)
        .map(|value| value == "agent_standup")
        .unwrap_or(false)
}

pub(crate) fn resolve_standup_report_path_template(
    automation: &AutomationV2Spec,
) -> Option<String> {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("standup"))
        .and_then(|value| value.get("report_path_template"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn resolve_standup_report_path_for_run(
    automation: &AutomationV2Spec,
    started_at_ms: u64,
) -> Option<String> {
    let template = resolve_standup_report_path_template(automation)?;
    if !template.contains("{{date}}") {
        return Some(template);
    }
    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(started_at_ms as i64)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m-%d")
        .to_string();
    Some(template.replace("{{date}}", &date))
}

pub(crate) fn automation_effective_required_output_path_for_run(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    run_id: &str,
    started_at_ms: u64,
) -> Option<String> {
    if is_agent_standup_automation(automation) && node.node_id == "standup_synthesis" {
        if let Some(path) = resolve_standup_report_path_for_run(automation, started_at_ms) {
            return Some(path);
        }
    }
    let runtime_values = automation_prompt_runtime_values(Some(started_at_ms));
    automation_node_required_output_path_with_runtime_for_run(
        node,
        Some(run_id),
        Some(&runtime_values),
    )
}

/// Derives the receipt path from the standup report path by inserting a
/// "receipt-" prefix on the filename and replacing the extension with ".json".
/// Example: "docs/standups/2026-04-05.md" → "docs/standups/receipt-2026-04-05.json"
pub(crate) fn standup_receipt_path_for_report(report_path: &str) -> String {
    let p = std::path::Path::new(report_path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("standup");
    let dir = p
        .parent()
        .and_then(|d| d.to_str())
        .filter(|d| !d.is_empty())
        .unwrap_or("docs/standups");
    format!("{dir}/receipt-{stem}.json")
}

/// Builds an operator-facing JSON receipt for a completed standup run.
/// Sources all data from existing structures: run checkpoint, lifecycle history,
/// node outputs, and the coordinator's assessment score.
/// Returns None if the run data is not available or this is not a standup run.
pub(crate) fn build_standup_run_receipt(
    run: &AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    run_id: &str,
    report_path: &str,
    coordinator_assessment: &ArtifactCandidateAssessment,
) -> Option<Value> {
    let completed_at_iso = run
        .finished_at_ms
        .or(run.started_at_ms)
        .map(|ms| {
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339()
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Count lifecycle events by type for summary
    let lifecycle_events = &run.checkpoint.lifecycle_history;
    let total_events = lifecycle_events.len();
    let total_repair_cycles = lifecycle_events
        .iter()
        .filter(|e| e.event == "node_repair_requested")
        .count();
    // Filler rejections are repair cycles on standup_update nodes
    let total_filler_rejections = lifecycle_events
        .iter()
        .filter(|e| {
            e.event == "node_repair_requested"
                && e.metadata
                    .as_ref()
                    .and_then(|m| m.get("contract_kind"))
                    .and_then(Value::as_str)
                    .is_some_and(|k| k == "standup_update")
        })
        .count();

    // Build per-participant summaries from node outputs
    let participants: Vec<Value> = automation
        .flow
        .nodes
        .iter()
        .filter(|n| n.node_id != "standup_synthesis")
        .map(|participant_node| {
            let node_output = run
                .checkpoint
                .node_outputs
                .get(&participant_node.node_id);
            let attempts = run
                .checkpoint
                .node_attempts
                .get(&participant_node.node_id)
                .copied()
                .unwrap_or(0);
            let status = node_output
                .and_then(|o| o.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            // Extract yesterday/today from the participant's standup JSON,
            // stored in the node output content text
            let standup_json = node_output
                .and_then(|o| o.get("content"))
                .and_then(|c| c.get("text").or_else(|| c.get("raw_assistant_text")))
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok());
            let yesterday = standup_json
                .as_ref()
                .and_then(|v| v.get("yesterday"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let today = standup_json
                .as_ref()
                .and_then(|v| v.get("today"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let filler_rejected = lifecycle_events.iter().any(|e| {
                e.event == "node_repair_requested"
                    && e.metadata
                        .as_ref()
                        .and_then(|m| m.get("node_id"))
                        .and_then(Value::as_str)
                        .is_some_and(|id| id == participant_node.node_id)
            });
            // Derive a readable name from the node_id (e.g., "participant_0_copywriter")
            let display_name = participant_node
                .node_id
                .splitn(3, '_')
                .nth(2)
                .unwrap_or(&participant_node.node_id)
                .replace('_', " ");
            json!({
                "node_id": participant_node.node_id,
                "display_name": display_name,
                "attempts": attempts,
                "status": status,
                "filler_rejected": filler_rejected,
                "yesterday_summary": if yesterday.is_empty() { Value::Null } else { json!(yesterday) },
                "today_summary": if today.is_empty() { Value::Null } else { json!(today) },
            })
        })
        .collect();

    let coordinator_attempts = run
        .checkpoint
        .node_attempts
        .get("standup_synthesis")
        .copied()
        .unwrap_or(0);

    Some(json!({
        "run_id": run_id,
        "automation_id": automation.automation_id,
        "automation_name": automation.name,
        "completed_at_iso": completed_at_iso,
        "report_path": report_path,
        "participants": participants,
        "coordinator": {
            "node_id": "standup_synthesis",
            "attempts": coordinator_attempts,
            "report_path": report_path,
            "assessment": assessment::artifact_candidate_summary(coordinator_assessment, true),
        },
        "lifecycle_event_count": total_events,
        "total_repair_cycles": total_repair_cycles,
        "total_filler_rejections": total_filler_rejections,
    }))
}
