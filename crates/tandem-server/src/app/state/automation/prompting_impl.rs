use super::*;

fn automation_prompt_html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn automation_prompt_render_path_bullets(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| format!("- `{}`", path))
        .collect::<Vec<_>>()
        .join("\n")
}

fn automation_prompt_apply_runtime_placeholders(
    text: &str,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> String {
    let Some(runtime_values) = runtime_values else {
        return text.to_string();
    };
    text.replace("{current_date}", &runtime_values.current_date)
        .replace("{current_time}", &runtime_values.current_time)
        .replace("{current_timestamp}", &runtime_values.current_timestamp)
}

fn automation_prompt_apply_runtime_placeholders_to_value(
    value: &Value,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> Value {
    match value {
        Value::String(text) => Value::String(automation_prompt_apply_runtime_placeholders(
            text,
            runtime_values,
        )),
        Value::Array(rows) => Value::Array(
            rows.iter()
                .map(|row| {
                    automation_prompt_apply_runtime_placeholders_to_value(row, runtime_values)
                })
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        automation_prompt_apply_runtime_placeholders_to_value(
                            value,
                            runtime_values,
                        ),
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn automation_prompt_render_canonical_html_body(text: &str) -> String {
    let trimmed = text.trim();
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
        if line.starts_with("- ") || line.starts_with("* ") {
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            let item = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line);
            html.push_str(&format!(
                "<li>{}</li>",
                automation_prompt_html_escape(item.trim())
            ));
            continue;
        }

        flush_list(&mut html, &mut in_list);
        html.push_str(&format!("<p>{}</p>", automation_prompt_html_escape(line)));
    }
    flush_list(&mut html, &mut in_list);
    html
}

/// Extracts the standup participant update JSON (`yesterday`/`today`/`blockers`)
/// from an upstream input value. The participant output is a full automation node
/// output object; the actual standup JSON may be in several places depending on
/// how the session text was captured.
///
/// Returns None if the input does not appear to be a standup participant output.
fn extract_standup_participant_update(input: &Value) -> Option<Value> {
    let output = input.get("output")?;
    let content = output.get("content")?;

    // Try content.text (parsed as JSON first, then as raw standup update)
    let text_candidates = [
        content.get("text").and_then(Value::as_str),
        content.get("raw_assistant_text").and_then(Value::as_str),
        content.get("raw_text").and_then(Value::as_str),
    ];
    for text in text_candidates.into_iter().flatten() {
        // Participant outputs are JSON with yesterday/today keys
        if let Ok(parsed) = serde_json::from_str::<Value>(text.trim()) {
            if parsed.get("yesterday").is_some() || parsed.get("today").is_some() {
                return Some(parsed);
            }
        }
    }
    // Try content.data if the JSON was already pre-parsed
    if let Some(data) = content.get("data") {
        if data.get("yesterday").is_some() || data.get("today").is_some() {
            return Some(data.clone());
        }
    }
    None
}

fn automation_prompt_extract_upstream_text(input: &Value) -> Option<String> {
    let mut candidates = Vec::new();
    for pointer in [
        "/output/content/text",
        "/output/content/raw_assistant_text",
        "/output/text",
        "/output/raw_text",
    ] {
        if let Some(text) = input.pointer(pointer).and_then(Value::as_str) {
            candidates.push(text.trim().to_string());
        }
    }
    if candidates.is_empty() {
        if let Some(output) = input.get("output") {
            if let Ok(rendered) = serde_json::to_string(output) {
                candidates.push(rendered);
            }
        }
    }
    candidates
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .max_by_key(|value| value.len())
}

fn automation_prompt_render_delivery_source_body(upstream_inputs: &[Value]) -> Option<String> {
    let mut best = upstream_inputs
        .iter()
        .filter_map(|input| {
            let text = automation_prompt_extract_upstream_text(input)?;
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
    let rendered_html = automation_prompt_render_canonical_html_body(&text);
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
    render_automation_v2_prompt_with_options(
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
        AutomationPromptRenderOptions::default(),
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
    let runtime_values = options.runtime_values.as_ref();
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("structured_json");
    let validator_kind = automation_output_validator_kind(node);
    let handoff_only_structured_json = validator_kind
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
    let normalized_upstream_inputs = upstream_inputs
        .iter()
        .map(|input| {
            let mut normalized_input = input.clone();
            if let Some(output) = input.get("output") {
                if let Some(object) = normalized_input.as_object_mut() {
                    object.insert(
                        "output".to_string(),
                        normalize_upstream_research_output_paths(
                            workspace_root,
                            Some(run_id),
                            output,
                        ),
                    );
                }
            }
            if let Some(output) = normalized_input.get_mut("output") {
                // Strip context-write IDs (e.g. `ctx:wfplan-...:node:artifact`) from ALL
                // upstream inputs for ALL node types. These internal engine references
                // look like file paths but are never valid write targets; models that see
                // them try to write to them, causing WRITE_PATH_REJECTED failures.
                automation_prompt_strip_context_writes(output);
            }
            automation_prompt_apply_runtime_placeholders_to_value(&normalized_input, runtime_values)
        })
        .collect::<Vec<_>>();
    let preserve_full_upstream_inputs = automation_node_preserves_full_upstream_inputs(node);
    let summary_only_upstream = options.summary_only_upstream && !preserve_full_upstream_inputs;
    let mut sections = Vec::new();
    if let Some(system_prompt) = template_system_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Template system prompt:\n{}", system_prompt));
    }
    if let Some(mission) = automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("mission"))
    {
        let mission_title = mission
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(automation.name.as_str());
        let mission_goal = automation_prompt_apply_runtime_placeholders(
            mission
                .get("goal")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            runtime_values,
        );
        let success_criteria = mission
            .get("success_criteria")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(|row| {
                        format!(
                            "- {}",
                            automation_prompt_apply_runtime_placeholders(
                                row.trim(),
                                runtime_values
                            )
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let shared_context = automation_prompt_apply_runtime_placeholders(
            mission
                .get("shared_context")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            runtime_values,
        );
        sections.push(format!(
            "Mission Brief:\nTitle: {mission_title}\nGoal: {mission_goal}\nShared context: {shared_context}\nSuccess criteria:\n{}",
            if success_criteria.is_empty() {
                "- none provided".to_string()
            } else {
                success_criteria
            }
        ));
    }
    if let Some(runtime_values) = runtime_values {
        sections.push(format!(
            "Resolved Runtime Values:\n- Use these exact values for this run.\n- `current_date` = `{}`\n- `current_time` = `{}`\n- `current_timestamp` = `{}`\n- Replace any literal `{{current_date}}`, `{{current_time}}`, and `{{current_timestamp}}` tokens in objectives, paths, or file contents before reading or writing workspace files.",
            runtime_values.current_date,
            runtime_values.current_time,
            runtime_values.current_timestamp,
        ));
    }
    sections.push(format!(
        "Automation ID: {}\nRun ID: {}\nNode ID: {}\nAgent: {}\nObjective: {}\nOutput contract kind: {}",
        automation.automation_id,
        run_id,
        node.node_id,
        agent.display_name,
        automation_prompt_apply_runtime_placeholders(&node.objective, runtime_values),
        contract_kind
    ));
    if let Some(contract) = node.output_contract.as_ref() {
        let schema = contract
            .schema
            .as_ref()
            .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
            .unwrap_or_else(|| "none".to_string());
        let guidance = contract.summary_guidance.as_deref().unwrap_or("none");
        sections.push(format!(
            "Output Contract:\nKind: {}\nSummary guidance: {}\nSchema:\n{}",
            contract.kind, guidance, schema
        ));
    }
    if let Some(builder) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
    {
        let local_title = builder
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(node.node_id.as_str());
        let local_prompt = automation_prompt_apply_runtime_placeholders(
            builder
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            runtime_values,
        );
        let local_role = automation_prompt_apply_runtime_placeholders(
            builder
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            runtime_values,
        );
        sections.push(format!(
            "Local Assignment:\nTitle: {local_title}\nRole: {local_role}\nInstructions: {local_prompt}"
        ));
    }
    let connector_discovery_text = {
        let local_prompt = node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(Value::as_object)
            .and_then(|builder| builder.get("prompt"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        format!(
            "{}\n{}",
            automation_prompt_apply_runtime_placeholders(&node.objective, runtime_values),
            automation_prompt_apply_runtime_placeholders(local_prompt, runtime_values)
        )
    };
    if tandem_plan_compiler::api::workflow_plan_should_surface_mcp_discovery(
        &connector_discovery_text,
        &agent.mcp_policy.allowed_servers,
    ) {
        sections.push(format!(
            "MCP Discovery:\n- MCP-backed work may be relevant for this node.\n- Allowed MCP servers: {}.\n- Call `mcp_list` before reading or comparing sources so you know which connector-backed tools are available.\n- Prefer MCP-backed tools for source-specific systems when the connector exists.\n- If the objective depends on a connector-backed source and no relevant MCP tool is available, finish the artifact from the local evidence you already have and record that limitation instead of repeating discovery calls.",
            serde_json::to_string_pretty(&agent.mcp_policy.allowed_servers)
                .unwrap_or_else(|_| "[]".to_string())
        ));
    }
    if let Some(inputs) = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("inputs"))
        .filter(|value| !value.is_null())
    {
        let rendered = serde_json::to_string_pretty(
            &automation_prompt_apply_runtime_placeholders_to_value(inputs, runtime_values),
        )
        .unwrap_or_else(|_| inputs.to_string());
        sections.push(format!(
            "Node Inputs:\n- Use these values directly when they satisfy the objective.\n- Do not search `/tmp`, shell history, or undeclared temp files for duplicate copies of these inputs.\n{}",
            rendered
                .lines()
                .map(|line| format!("  {}", line))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let execution_mode = automation_node_execution_mode(node, workspace_root);
    let required_output_path = automation_node_required_output_path_for_run(node, Some(run_id));
    let required_workspace_write_targets =
        automation_node_must_write_files_for_automation(automation, node, runtime_values)
            .into_iter()
            .map(|path| automation_prompt_apply_runtime_placeholders(&path, runtime_values))
            .filter(|path| {
                required_output_path
                    .as_ref()
                    .is_none_or(|output_path| path != output_path)
            })
            .collect::<Vec<_>>();
    let write_scope_rule = if required_workspace_write_targets.is_empty() {
        "- Use only declared workflow artifact paths.".to_string()
    } else {
        format!(
            "- Use only approved write targets for this node: the declared run artifact plus these required workspace files: {}.",
            required_workspace_write_targets
                .iter()
                .map(|path| format!("`{}`", path))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    sections.push(format!(
        "Execution Policy:\n- Mode: `{}`.\n{}\n- Create only parent folders as directories; treat paths ending in file-like suffixes such as `.md`, `.json`, `.jsonl`, `.yaml`, `.yml`, `.toml`, `.txt`, or `.csv` as files.\n- Do not use `bash`/`mkdir` to create a file path itself; use `write` with the full file contents when a file must be created.\n- Keep status and blocker notes in the response JSON, not as placeholder file contents.",
        execution_mode,
        write_scope_rule
    ));
    if let Some(output_path) = required_output_path.as_ref() {
        sections.push(format!(
            "Artifact Delivery Order:\n- If MCP Discovery is present, call `mcp_list` before reading or comparing sources so you know which connector-backed tools are available.\n- Read or inspect the concrete sources required by the node.\n- Write the required run artifact to `{}` before ending this attempt.\n- On retries, rewrite the file in the current attempt even if the content is identical.\n- Do not stop with only a chat summary; the file is the deliverable.",
            output_path
        ));
        sections.push(
            "Artifact Delivery Fallback:\n- If discovery does not reveal a useful connector-backed source, finish the artifact from the local evidence you already have and record that limitation in the file instead of repeating discovery calls."
                .to_string(),
        );
    }
    if automation_node_is_code_workflow(node) {
        let task_kind =
            automation_node_task_kind(node).unwrap_or_else(|| "code_change".to_string());
        let project_backlog_tasks = automation_node_projects_backlog_tasks(node);
        let task_id = automation_node_task_id(node).unwrap_or_else(|| "unassigned".to_string());
        let repo_root = automation_node_repo_root(node).unwrap_or_else(|| ".".to_string());
        let write_scope =
            automation_node_write_scope(node).unwrap_or_else(|| "repo-scoped edits".to_string());
        let acceptance_criteria = automation_node_acceptance_criteria(node)
            .unwrap_or_else(|| "satisfy the declared coding task acceptance criteria".to_string());
        let task_dependencies =
            automation_node_task_dependencies(node).unwrap_or_else(|| "none declared".to_string());
        let verification_state =
            automation_node_verification_state(node).unwrap_or_else(|| "pending".to_string());
        let task_owner =
            automation_node_task_owner(node).unwrap_or_else(|| "unclaimed".to_string());
        let verification_command =
            automation_node_verification_command(node).unwrap_or_else(|| {
                "run the most relevant repo-local build, test, or lint commands".to_string()
            });
        sections.push(format!(
            "Coding Task Context:\n- Task id: `{}`.\n- Task kind: `{}`.\n- Repo root: `{}`.\n- Declared write scope: {}.\n- Acceptance criteria: {}.\n- Backlog dependencies: {}.\n- Verification state: {}.\n- Preferred owner: {}.\n- Verification expectation: {}.\n- Projects backlog tasks: {}.\n- Prefer repository edits plus a concise handoff artifact, not placeholder file rewrites.\n- Use `bash` for verification commands when tool access allows it.",
            task_id, task_kind, repo_root, write_scope, acceptance_criteria, task_dependencies, verification_state, task_owner, verification_command, if project_backlog_tasks { "yes" } else { "no" }
        ));
    }
    if let Some(output_path) = required_output_path {
        let approved_write_targets_rule = if required_workspace_write_targets.is_empty() {
            "- Only write declared workflow artifact files.".to_string()
        } else {
            format!(
                "- In addition to the run artifact, create or update these required workspace files when needed: {}.\n- Do not create other auxiliary touch files, status files, marker files, or placeholder preservation notes.",
                required_workspace_write_targets
                    .iter()
                    .map(|path| format!("`{}`", path))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let output_rules = match execution_mode {
            "git_patch" => format!(
                "Required Run Artifact:\n- Create or update `{}` for this run.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `apply_patch` for multi-line source edits and `edit` for localized replacements.\n- Use `write` only for brand-new files or when patch/edit cannot express the change.\n- Do not replace an existing source file with a status note, preservation note, or placeholder summary.\n{}\n- Do not report success unless this run artifact exists when the stage ends.",
                output_path,
                approved_write_targets_rule
            ),
            "filesystem_patch" => format!(
                "Required Run Artifact:\n- Create or update `{}` for this run.\n- Use `glob` to discover candidate paths and `read` only for concrete file paths.\n- Prefer `edit` for existing-file changes.\n- Use `write` for brand-new files or as a last resort when an edit cannot express the change.\n- Do not replace an existing file with a status note, preservation note, or placeholder summary.\n{}\n- Do not report success unless this run artifact exists when the stage ends.",
                output_path,
                approved_write_targets_rule
            ),
            _ => format!(
                "Required Run Artifact:\n- Create or update `{}` for this run.\n- When calling the `write` tool, include the full file body in the `content` field; do not call `write` with only a path or an empty body.\n- If this output path is already known, use `write` immediately to create the full file contents.\n- On every retry attempt, rewrite the required output in this attempt even if the content would be identical; do not rely on a prior attempt’s file.\n- Use `glob` and `read` only when you must inspect existing companion files or verify a preexisting artifact before updating it.\n- Do not let an empty `glob` end the run; still create the required artifact.\n{}\n- Overwrite the declared output with the actual artifact contents for this run instead of preserving a prior placeholder.\n- If the required run artifact is JSON, also include the exact JSON artifact body in the final response before the compact status object so the engine can recover the artifact when provider-side write delivery is flaky.\n- Do not report success unless this run artifact exists when the stage ends.",
                output_path,
                approved_write_targets_rule
            ),
        };
        sections.push(output_rules);
    }
    if !required_workspace_write_targets.is_empty() {
        sections.push(format!(
            "Required Workspace Writes:\n- These workspace files are part of the node objective and are approved write targets for this run.\n{}\n- Keep these writes inside the workspace root and use full file contents when creating a file.",
            automation_prompt_render_path_bullets(&required_workspace_write_targets)
        ));
    }
    let triage_gate = node
        .metadata
        .as_ref()
        .and_then(|m| m.get("triage_gate"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if triage_gate && automation_node_required_output_path(node).is_none() {
        sections.push(
            "Triage Workspace Inspection:\n- Use `glob` to probe required folders and expected bootstrap files.\n- Use `read` on concrete files when needed to decide `has_work`.\n- Do not include prose; return only the structured JSON handoff plus the final compact status object."
                .to_string(),
        );
    }
    if automation_node_web_research_expected(node) {
        let requested_has_websearch = requested_tools.iter().any(|tool| tool == "websearch");
        let requested_has_webfetch = requested_tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "webfetch" | "webfetch_html"));
        let next_step_hint = automation_node_required_output_path(node).map(|p| {
            format!(
                "Next Step:\n- Call `websearch` now (2–3 focused queries), optionally `webfetch` top result URLs for details, then call `write` to create `{}` before ending this attempt.\n- Do not end the attempt without at least one productive tool call when a run artifact is required.",
                p
            )
        });
        if requested_has_websearch {
            sections.push(
                "External Research Expectation:\n- Use `websearch` for current external evidence before finalizing the output file.\n- Use `webfetch` on concrete result URLs when search snippets are not enough.\n- Include only evidence you can support from local files or current web findings.\n- If `websearch` returns an authorization-required or unavailable result, treat external research as unavailable for this run, continue with local file reads, and note the web-research limitation instead of stopping."
                    .to_string(),
            );
            if let Some(hint) = next_step_hint {
                sections.push(hint);
            }
        } else if requested_has_webfetch {
            sections.push(
                "External Research Expectation:\n- `websearch` is not available in this run.\n- Use `webfetch` only for concrete URLs already present in local sources or upstream handoffs.\n- If you cannot validate externally without search, record that limitation in the structured handoff and finish the node.\n- Do not ask the user for clarification or permission to continue; return the required JSON handoff for this run."
                    .to_string(),
            );
            if let Some(hint) = next_step_hint {
                sections.push(hint);
            }
        } else {
            sections.push(
                "External Research Expectation:\n- No web research tool is available in this run.\n- Record the web-research limitation clearly in the structured handoff, continue with any allowed local reads, and finish without asking follow-up questions."
                    .to_string(),
            );
        }
    }
    if handoff_only_structured_json {
        sections.push(
            "Structured Handoff Expectation:\n- Return the requested structured JSON handoff in the final response body.\n- The final response body should contain JSON only: the handoff JSON, then the final compact JSON status object.\n- Do not include headings, bullets, markdown fences, prose explanations, or follow-up questions.\n- Do not stop after tool calls alone; include a machine-readable JSON object or array with the requested fields.\n- Treat any `ctx:...` values or `step_context_bindings` metadata as internal context identifiers, not filesystem paths.\n- Do not call `write` unless this node explicitly declares a workflow output path."
                .to_string(),
        );
    }
    let mut prompt = sections.join("\n\n");
    if !normalized_upstream_inputs.is_empty() {
        // For standup coordinator nodes, format participant outputs as structured
        // labeled sections (Yesterday / Today / Blockers) rather than raw JSON.
        // This reduces coordinator confusion from pipeline metadata fields and
        // surfaces the actual standup content in a human-readable form.
        let is_standup_coordinator = node.node_id == "standup_synthesis"
            && normalized_upstream_inputs.iter().any(|input| {
                // Participant outputs contain standup JSON with yesterday/today keys
                extract_standup_participant_update(input).is_some()
            });

        if is_standup_coordinator {
            prompt.push_str("\n\nStandup Participant Updates:");
            for input in &normalized_upstream_inputs {
                let alias = input
                    .get("alias")
                    .and_then(Value::as_str)
                    .unwrap_or("participant");
                // Skip non-participant synthetic inputs like runtime_context_partition
                if alias == "runtime_context_partition" || alias == "runtime_credential_envelope" {
                    continue;
                }
                let display_name = alias
                    .splitn(3, '_')
                    .nth(2)
                    .unwrap_or(alias)
                    .replace('_', " ");
                if let Some(update) = extract_standup_participant_update(input) {
                    let yesterday = update
                        .get("yesterday")
                        .and_then(Value::as_str)
                        .unwrap_or("(none reported)")
                        .trim();
                    let today = update
                        .get("today")
                        .and_then(Value::as_str)
                        .unwrap_or("(none reported)")
                        .trim();
                    let blockers = update
                        .get("blockers")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .unwrap_or("none");
                    let status = update
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    prompt.push_str(&format!(
                        "\n\n### {display_name} (node: {alias}, status: {status})\
                         \n- Yesterday: {yesterday}\
                         \n- Today: {today}\
                         \n- Blockers: {blockers}"
                    ));
                } else {
                    // Participant did not produce a valid standup update — note it
                    let status = input
                        .get("output")
                        .and_then(|o| o.get("status"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    prompt.push_str(&format!(
                        "\n\n### {display_name} (node: {alias}, status: {status})\
                         \n- No standup update produced."
                    ));
                }
            }
        } else {
            prompt.push_str("\n\nUpstream Inputs:");
            for input in &normalized_upstream_inputs {
                let alias = input
                    .get("alias")
                    .and_then(Value::as_str)
                    .unwrap_or("input");
                let from_step_id = input
                    .get("from_step_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let output = input
                    .get("output")
                    .map(|value| {
                        compact_automation_prompt_output_with_mode(value, summary_only_upstream)
                    })
                    .unwrap_or(Value::Null);
                let rendered =
                    serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string());
                prompt.push_str(&format!(
                    "\n- {}\n  from_step_id: {}\n  output:\n{}",
                    alias,
                    from_step_id,
                    rendered
                        .lines()
                        .map(|line| format!("    {}", line))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }
    }
    if automation_node_is_research_finalize(node) {
        if let Some(summary) =
            render_research_finalize_upstream_summary(&normalized_upstream_inputs)
        {
            prompt.push_str("\n\n");
            prompt.push_str(&summary);
        }
    }
    if let Some(summary) =
        render_upstream_synthesis_guidance(node, &normalized_upstream_inputs, run_id)
    {
        prompt.push_str("\n\n");
        prompt.push_str(&summary);
    }
    if automation_node_requires_email_delivery(node) {
        prompt.push_str(
            "\n\nDelivery rules:\n- Prefer inline email body delivery by default.\n- Only include an email attachment when upstream inputs contain a concrete attachment artifact with a non-empty s3key or upload result.\n- Never send an attachment parameter with an empty or null s3key.\n- If no attachment artifact exists, omit the attachment parameter entirely.",
        );
        let delivery_target =
            automation_node_delivery_target(node).unwrap_or_else(|| "missing".to_string());
        let content_type =
            automation_node_email_content_type(node).unwrap_or_else(|| "text/html".to_string());
        let inline_body_only = automation_node_inline_body_only(node).unwrap_or(true);
        let attachments_allowed = automation_node_allows_attachments(node).unwrap_or(false);
        prompt.push_str(&format!(
            "\n\nDelivery target:\n- Method: `email`\n- Recipient: `{}`\n- Content-Type: `{}`\n- Inline body only: `{}`\n- Attachments allowed: `{}`\n- Treat this delivery target as authoritative for this run.\n- Do not say the recipient is missing when it is listed above.\n- Do not mark the node completed unless you actually execute an email draft or send tool.",
            delivery_target,
            content_type,
            inline_body_only,
            attachments_allowed
        ));
        if let Some(delivery_source) =
            automation_prompt_render_delivery_source_body(&normalized_upstream_inputs)
        {
            prompt.push_str("\n\n");
            prompt.push_str(&delivery_source);
        }
    }
    if let Some(report_path) = standup_report_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nStandup report path:\n- Write the final markdown report to `{}` relative to the workspace root.\n- Use the `write` tool for the report.\n- The report must remain inside the workspace.",
            report_path
        ));
    }
    if let Some(project_id) = memory_project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(&format!(
            "\n\nMemory search scope:\n- `memory_search` defaults to the current session, current project, and global memory.\n- Current project_id: `{}`.\n- Use `tier: \"project\"` when you need recall limited to this workspace.\n- Use workspace files via `glob`, `grep`, and `read` when memory is sparse or stale.",
            project_id
        ));
    }
    if let Some(knowledge_context) = options
        .knowledge_context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str("\n\n");
        prompt.push_str(knowledge_context);
    }
    let enforce_completed_first_attempt = (validator_kind
        == crate::AutomationOutputValidatorKind::ResearchBrief
        || !automation_node_required_tools(node).is_empty()
        || handoff_only_structured_json)
        && attempt <= 1;
    if enforce_completed_first_attempt {
        if automation_node_required_output_path(node).is_some() {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the write tool actually created the output file.",
            );
        } else {
            prompt.push_str(
                "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with `status` set to `completed`.\n- Do not declare the output blocked while the required workflow tools remain available; use them first and finish the work.\n- Do not claim success unless the required structured handoff was actually returned in the final response.",
            );
        }
    } else if handoff_only_structured_json {
        prompt.push_str(
            "\n\nFinal response requirements:\n- Return a concise completion.\n- Include the required structured handoff JSON in the response body before the final compact status object.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim success unless the required structured handoff was actually returned in the final response.\n- Do not claim semantic success if the output is blocked or not approved.",
        );
    } else {
        prompt.push_str(
            "\n\nFinal response requirements:\n- Return a concise completion.\n- Include a final compact JSON object in the response body with at least `status` (`completed` or `blocked`).\n- For review-style nodes, also include `approved` (`true` or `false`).\n- If blocked, include a short `reason`.\n- Do not claim semantic success if the output is blocked or not approved.",
        );
    }
    prompt
}

fn truncate_automation_prompt_text(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let truncated = trimmed.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}

fn compact_automation_prompt_content(content: &Value, summary_only: bool) -> Value {
    let Some(object) = content.as_object() else {
        return content.clone();
    };
    let mut compact = serde_json::Map::new();
    if let Some(path) = object.get("path").cloned().filter(|value| !value.is_null()) {
        compact.insert("path".to_string(), path);
    }
    if let Some(handoff) = object
        .get("structured_handoff")
        .cloned()
        .filter(|value| !value.is_null())
    {
        compact.insert("structured_handoff".to_string(), handoff);
        return Value::Object(compact);
    }
    let candidate_text = object
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| object.get("raw_text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(text) = candidate_text {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if summary_only {
                compact.insert("data_summary".to_string(), summarize_json_keys(&parsed));
            } else {
                compact.insert("data".to_string(), parsed);
            }
        } else {
            compact.insert(
                "text".to_string(),
                json!(truncate_automation_prompt_text(
                    text,
                    if summary_only { 800 } else { 4000 }
                )),
            );
        }
    }
    Value::Object(compact)
}

fn automation_prompt_strip_context_writes(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("context_writes");
            for (key, child) in object.iter_mut() {
                if matches!(key.as_str(), "text" | "raw_text" | "raw_assistant_text") {
                    if let Some(raw) = child.as_str() {
                        if let Ok(mut parsed) = serde_json::from_str::<Value>(raw) {
                            automation_prompt_strip_context_writes(&mut parsed);
                            *child = Value::String(parsed.to_string());
                            continue;
                        }
                    }
                }
                automation_prompt_strip_context_writes(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                automation_prompt_strip_context_writes(item);
            }
        }
        _ => {}
    }
}

fn compact_automation_prompt_output_with_mode(output: &Value, summary_only: bool) -> Value {
    let Some(object) = output.as_object() else {
        return output.clone();
    };
    let mut compact = serde_json::Map::new();
    for key in [
        "status",
        "phase",
        "contract_kind",
        "summary",
        "blocked_reason",
        "workflow_class",
    ] {
        if let Some(value) = object.get(key).cloned().filter(|value| !value.is_null()) {
            compact.insert(key.to_string(), value);
        }
    }
    if let Some(validator_summary) = object.get("validator_summary").and_then(Value::as_object) {
        let mut validator = serde_json::Map::new();
        for key in [
            "kind",
            "outcome",
            "warning_count",
            "warning_requirements",
            "unmet_requirements",
            "validation_basis",
        ] {
            if let Some(value) = validator_summary
                .get(key)
                .cloned()
                .filter(|value| !value.is_null())
            {
                validator.insert(key.to_string(), value);
            }
        }
        if !validator.is_empty() {
            compact.insert("validator_summary".to_string(), Value::Object(validator));
        }
    }
    if let Some(artifact_validation) = object.get("artifact_validation").and_then(Value::as_object)
    {
        let mut validation = serde_json::Map::new();
        for key in [
            "accepted_artifact_path",
            "accepted_candidate_source",
            "validation_outcome",
            "validation_profile",
            "warning_count",
            "warning_requirements",
            "unmet_requirements",
            "semantic_block_reason",
            "rejected_artifact_reason",
            "validation_basis",
        ] {
            if let Some(value) = artifact_validation
                .get(key)
                .cloned()
                .filter(|value| !value.is_null())
            {
                validation.insert(key.to_string(), value);
            }
        }
        if !validation.is_empty() {
            compact.insert("artifact_validation".to_string(), Value::Object(validation));
        }
    }
    if let Some(content) = object.get("content") {
        let compact_content = compact_automation_prompt_content(content, summary_only);
        if compact_content
            .as_object()
            .is_some_and(|value| !value.is_empty())
        {
            compact.insert("content".to_string(), compact_content);
        }
    }
    Value::Object(compact)
}

/// Test-accessible shim for `extract_standup_participant_update`.
/// Only compiled in test builds; production code uses the private function directly.
#[cfg(test)]
pub(crate) fn extract_standup_participant_update_pub(input: &Value) -> Option<Value> {
    extract_standup_participant_update(input)
}
