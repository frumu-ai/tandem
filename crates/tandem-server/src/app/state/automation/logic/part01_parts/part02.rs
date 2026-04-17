fn automation_prompt_render_canonical_html_body(text: &str, run_id: &str) -> String {
    let trimmed = automation_prompt_canonicalize_artifact_hrefs(text, run_id);
    if trimmed.is_empty() {
        return "<p></p>".to_string();
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.contains("<html") || lowered.contains("<body") || lowered.contains("<div") {
        return trimmed.to_string();
    }

    let mut html = String::new();
    let mut in_list = false;
    let flush_list = |html: &mut String, in_list: &mut bool| {
        if *in_list {
            html.push_str("</ul>");
            *in_list = false;
        }
    };

    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush_list(&mut html, &mut in_list);
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h3>{}</h3>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h2>{}</h2>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            flush_list(&mut html, &mut in_list);
            html.push_str(&format!(
                "<h1>{}</h1>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            html.push_str(&format!(
                "<li>{}</li>",
                automation_prompt_html_escape(rest.trim())
            ));
            continue;
        }
        flush_list(&mut html, &mut in_list);
        html.push_str(&format!("<p>{}</p>", automation_prompt_html_escape(line)));
    }
    flush_list(&mut html, &mut in_list);
    if html.is_empty() {
        "<p></p>".to_string()
    } else {
        html
    }
}

fn render_deterministic_delivery_body(upstream_inputs: &[Value], run_id: &str) -> Option<String> {
    let mut best = upstream_inputs
        .iter()
        .filter_map(|input| {
            let text = input
                .get("output")
                .and_then(|output| output.get("content"))
                .and_then(|content| content.get("text"))
                .or_else(|| input.get("text"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)?;
            let path = input
                .get("output")
                .and_then(|output| output.get("content"))
                .and_then(|content| content.get("path"))
                .or_else(|| input.get("path"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "upstream artifact".to_string());
            Some((text, path))
        })
        .collect::<Vec<_>>();
    best.sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.1.cmp(&right.1)));
    let (text, source_path) = best.into_iter().next()?;
    let source_path =
        automation_run_scoped_output_path(run_id, &source_path).unwrap_or(source_path);
    let rendered_html = automation_prompt_render_canonical_html_body(&text, run_id);
    Some(format!(
        "Deterministic Delivery Body:\n- Source artifact: `{}`\n- Canonical HTML body:\n{}\n- Use this exact body as the delivery source of truth.\n- Do not rewrite the body into a shorter teaser or substitute a fresh summary.",
        source_path,
        rendered_html
            .lines()
            .map(|line| format!("  {}", line))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn automation_phase_execution_mode_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, String> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .filter_map(|phase| {
                    let phase_id = phase.get("phase_id").and_then(Value::as_str)?.trim();
                    if phase_id.is_empty() {
                        return None;
                    }
                    let mode = phase
                        .get("execution_mode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("soft");
                    Some((phase_id.to_string(), mode.to_string()))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_current_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Option<(String, usize, String)> {
    let phase_rank = automation_phase_rank_map(automation);
    if phase_rank.is_empty() {
        return None;
    }
    let phase_modes = automation_phase_execution_mode_map(automation);
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| !completed.contains(&node.node_id))
        .filter_map(|node| {
            automation_node_builder_metadata(node, "phase_id").and_then(|phase_id| {
                phase_rank
                    .get(&phase_id)
                    .copied()
                    .map(|rank| (phase_id, rank))
            })
        })
        .min_by_key(|(_, rank)| *rank)
        .map(|(phase_id, rank)| {
            let mode = phase_modes
                .get(&phase_id)
                .cloned()
                .unwrap_or_else(|| "soft".to_string());
            (phase_id, rank, mode)
        })
}

pub(crate) fn automation_phase_rank_map(
    automation: &AutomationV2Spec,
) -> std::collections::HashMap<String, usize> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("phases"))
        .and_then(Value::as_array)
        .map(|phases| {
            phases
                .iter()
                .enumerate()
                .filter_map(|(index, phase)| {
                    phase
                        .get("phase_id")
                        .and_then(Value::as_str)
                        .map(|phase_id| (phase_id.to_string(), index))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn automation_node_sort_key(
    node: &AutomationFlowNode,
    phase_rank: &std::collections::HashMap<String, usize>,
    current_open_phase_rank: Option<usize>,
) -> (usize, usize, i32, String) {
    let phase_order = automation_node_builder_metadata(node, "phase_id")
        .as_ref()
        .and_then(|phase_id| phase_rank.get(phase_id))
        .copied()
        .unwrap_or(usize::MAX / 2);
    let open_phase_bias = current_open_phase_rank
        .map(|open_rank| usize::from(phase_order != open_rank))
        .unwrap_or(0);
    (
        open_phase_bias,
        phase_order,
        -node_runtime_impl::automation_node_builder_priority(node),
        node.node_id.clone(),
    )
}

pub(crate) fn automation_filter_runnable_by_open_phase(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    runnable: Vec<AutomationFlowNode>,
) -> Vec<AutomationFlowNode> {
    let Some((_, open_rank, _)) = automation_current_open_phase(automation, run) else {
        return runnable;
    };
    let phase_rank = automation_phase_rank_map(automation);
    let in_open_phase = runnable
        .iter()
        .filter(|node| {
            automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied()
                == Some(open_rank)
        })
        .cloned()
        .collect::<Vec<_>>();
    if in_open_phase.is_empty() {
        runnable
    } else {
        in_open_phase
    }
}

fn automation_plan_package(automation: &AutomationV2Spec) -> Option<compiler_api::PlanPackage> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("plan_package"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub(crate) fn automation_filter_runnable_by_routine_dependencies(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    runnable: Vec<AutomationFlowNode>,
) -> Vec<AutomationFlowNode> {
    runnable
        .into_iter()
        .filter(|node| {
            !node_runtime_impl::automation_node_routine_dependencies_blocked(automation, run, node)
        })
        .collect()
}

fn normalize_write_scope_entries(scope: Option<String>) -> Vec<String> {
    let Some(scope) = scope else {
        return vec!["__repo__".to_string()];
    };
    let entries = scope
        .split(|ch| matches!(ch, ',' | '\n' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('/').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        vec!["__repo__".to_string()]
    } else {
        entries
    }
}

fn write_scope_entries_conflict(left: &[String], right: &[String]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            a == "__repo__"
                || b == "__repo__"
                || a == b
                || a == "."
                || b == "."
                || a == "*"
                || b == "*"
                || a.starts_with(&format!("{}/", b))
                || b.starts_with(&format!("{}/", a))
        })
    })
}

pub(crate) fn automation_filter_runnable_by_write_scope_conflicts(
    runnable: Vec<AutomationFlowNode>,
    max_parallel: usize,
) -> Vec<AutomationFlowNode> {
    if max_parallel <= 1 {
        return runnable.into_iter().take(1).collect();
    }
    let mut selected = Vec::new();
    let mut selected_scopes = Vec::<Vec<String>>::new();
    for node in runnable {
        let is_code = automation_node_is_code_workflow(&node);
        let scope_entries = if is_code {
            normalize_write_scope_entries(automation_node_write_scope(&node))
        } else {
            Vec::new()
        };
        let conflicts = is_code
            && selected.iter().enumerate().any(|(index, existing)| {
                automation_node_is_code_workflow(existing)
                    && write_scope_entries_conflict(&scope_entries, &selected_scopes[index])
            });
        if conflicts {
            continue;
        }
        if is_code {
            selected_scopes.push(scope_entries);
        } else {
            selected_scopes.push(Vec::new());
        }
        selected.push(node);
        if selected.len() >= max_parallel {
            break;
        }
    }
    selected
}

pub(crate) fn automation_blocked_nodes(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> Vec<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let pending = run
        .checkpoint
        .pending_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let phase_rank = automation_phase_rank_map(automation);
    let current_open_phase = automation_current_open_phase(automation, run);
    automation
        .flow
        .nodes
        .iter()
        .filter(|node| pending.contains(&node.node_id))
        .filter_map(|node| {
            let missing_deps = node.depends_on.iter().any(|dep| !completed.contains(dep));
            if missing_deps {
                return Some(node.node_id.clone());
            }
            if node_runtime_impl::automation_node_routine_dependencies_blocked(
                automation, run, node,
            ) {
                return Some(node.node_id.clone());
            }
            let Some((_, open_rank, mode)) = current_open_phase.as_ref() else {
                return None;
            };
            if mode != "barrier" {
                return None;
            }
            let node_phase_rank = automation_node_builder_metadata(node, "phase_id")
                .as_ref()
                .and_then(|phase_id| phase_rank.get(phase_id))
                .copied();
            if node_phase_rank.is_some_and(|rank| rank > *open_rank) {
                return Some(node.node_id.clone());
            }
            None
        })
        .collect::<Vec<_>>()
}

pub(crate) fn record_automation_open_phase_event(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    let Some((phase_id, phase_rank, execution_mode)) =
        automation_current_open_phase(automation, run)
    else {
        return;
    };
    let last_recorded = run
        .checkpoint
        .lifecycle_history
        .iter()
        .rev()
        .find(|entry| entry.event == "phase_opened")
        .and_then(|entry| entry.metadata.as_ref())
        .and_then(|metadata| metadata.get("phase_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if last_recorded.as_deref() == Some(phase_id.as_str()) {
        return;
    }
    record_automation_lifecycle_event_with_metadata(
        run,
        "phase_opened",
        Some(format!("phase `{}` is now open", phase_id)),
        None,
        Some(json!({
            "phase_id": phase_id,
            "phase_rank": phase_rank,
            "execution_mode": execution_mode,
        })),
    );
}

pub fn refresh_automation_runtime_state(
    automation: &AutomationV2Spec,
    run: &mut AutomationV2RunRecord,
) {
    run.checkpoint.blocked_nodes = automation_blocked_nodes(automation, run);
    record_automation_open_phase_event(automation, run);
}

fn automation_mission_milestones(automation: &AutomationV2Spec) -> Vec<Value> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("mission"))
        .and_then(|mission| mission.get("milestones"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn completed_mission_milestones(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
) -> std::collections::HashSet<String> {
    let completed = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    automation_mission_milestones(automation)
        .iter()
        .filter_map(|milestone| {
            let milestone_id = milestone
                .get("milestone_id")
                .and_then(Value::as_str)?
                .trim();
            if milestone_id.is_empty() {
                return None;
            }
            let required = milestone
                .get("required_stage_ids")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (!required.is_empty()
                && required
                    .iter()
                    .all(|stage_id| completed.contains(*stage_id)))
            .then_some(milestone_id.to_string())
        })
        .collect()
}

pub(crate) fn record_milestone_promotions(
    automation: &AutomationV2Spec,
    row: &mut AutomationV2RunRecord,
    promoted_by_node_id: &str,
) {
    let already_recorded = row
        .checkpoint
        .lifecycle_history
        .iter()
        .filter(|entry| entry.event == "milestone_promoted")
        .filter_map(|entry| {
            entry.metadata.as_ref().and_then(|metadata| {
                metadata
                    .get("milestone_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .collect::<std::collections::HashSet<_>>();
    let completed = completed_mission_milestones(automation, row);
    for milestone in automation_mission_milestones(automation) {
        let milestone_id = milestone
            .get("milestone_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if milestone_id.is_empty()
            || !completed.contains(milestone_id)
            || already_recorded.contains(milestone_id)
        {
            continue;
        }
        let title = milestone
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or(milestone_id);
        let phase_id = milestone
            .get("phase_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let required_stage_ids = milestone
            .get("required_stage_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        record_automation_lifecycle_event_with_metadata(
            row,
            "milestone_promoted",
            Some(format!("milestone `{title}` promoted")),
            None,
            Some(json!({
                "milestone_id": milestone_id,
                "title": title,
                "phase_id": phase_id,
                "required_stage_ids": required_stage_ids,
                "promoted_by_node_id": promoted_by_node_id,
            })),
        );
    }
}

pub fn collect_automation_descendants(
    automation: &AutomationV2Spec,
    root_ids: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut descendants = root_ids.clone();
    let mut changed = true;
    while changed {
        changed = false;
        for node in &automation.flow.nodes {
            if descendants.contains(&node.node_id) {
                continue;
            }
            if node.depends_on.iter().any(|dep| descendants.contains(dep)) {
                descendants.insert(node.node_id.clone());
                changed = true;
            }
        }
    }
    descendants
}

/// Returns all transitive ancestors of `node_id` (i.e. every node that
/// `node_id` directly or indirectly depends on), not including `node_id`
/// itself.
pub fn collect_automation_ancestors(
    automation: &AutomationV2Spec,
    node_id: &str,
) -> std::collections::HashSet<String> {
    let mut ancestors = std::collections::HashSet::new();
    let mut queue = vec![node_id.to_string()];
    while let Some(current_id) = queue.pop() {
        if let Some(node) = automation
            .flow
            .nodes
            .iter()
            .find(|n| n.node_id == current_id)
        {
            for dep in &node.depends_on {
                if ancestors.insert(dep.clone()) {
                    queue.push(dep.clone());
                }
            }
        }
    }
    ancestors
}

pub(crate) fn render_automation_v2_prompt(
    automation: &AutomationV2Spec,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
    requested_tools: &[String],
    template_system_prompt: Option<&str>,
    standup_report_path: Option<&str>,
    memory_project_id: Option<&str>,
) -> String {
    prompting_impl::render_automation_v2_prompt(
        automation,
        workspace_root,
        run_id,
        node,
        attempt,
        agent,
        upstream_inputs,
        requested_tools,
        template_system_prompt,
        standup_report_path,
        memory_project_id,
    )
}

pub(crate) fn render_automation_v2_prompt_with_options(
    automation: &AutomationV2Spec,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    attempt: u32,
    agent: &AutomationAgentProfile,
    upstream_inputs: &[Value],
    requested_tools: &[String],
    template_system_prompt: Option<&str>,
    standup_report_path: Option<&str>,
    memory_project_id: Option<&str>,
    options: AutomationPromptRenderOptions,
) -> String {
    prompting_impl::render_automation_v2_prompt_with_options(
        automation,
        workspace_root,
        run_id,
        node,
        attempt,
        agent,
        upstream_inputs,
        requested_tools,
        template_system_prompt,
        standup_report_path,
        memory_project_id,
        options,
    )
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
            "\n\nFINAL ATTEMPT:\n- This is the last retry.\n- The engine will accept the output file at `{}` as-is if it exists when this attempt ends.\n- Do not ask follow-up questions.\n- Do not end with a summary.\n- Write the complete artifact to the output path and include {{\"status\":\"completed\"}} as the last line of your response.",
            output_path
        )
    } else {
        String::new()
    };

    Some(format!(
        "Repair Brief:\n- Node `{}` is being retried because the previous attempt ended in `needs_repair`.\n- Previous validation reason: {}.\n- Validation basis: {}.\n- Upstream read paths available for synthesis: {}.\n- Required source read paths: {}.\n- Missing required source read paths: {}.\n- Unmet requirements: {}.\n- Blocking classification: {}.\n- Required next tool actions: {}.\n- Tools offered last attempt: {}.\n- Tools executed last attempt: {}.\n- Relevant files still unread or explicitly unreviewed: {}.\n- Previous repair attempt count: {}.\n- Remaining repair attempts after this run: {}{}.\n- For this retry, satisfy the unmet requirements before finalizing the artifact.\n- Do not write a blocked handoff unless the required tools were actually attempted and remained unavailable or failed.{}",
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
    ))
}
